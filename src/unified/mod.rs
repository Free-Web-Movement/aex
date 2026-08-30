//! # Unified Protocol Server
//!
//! Unified server supporting HTTP/1.1, HTTP/2, WebSocket, TCP, and UDP protocols on the same port.
//!
//! Protocol identification is performed by pluggable [`detect`]ors held in a
//! [`DetectorRegistry`]. Built-in HTTP/1.1 and HTTP/2 detectors are registered
//! by default; additional detectors can be added, removed, reordered, or
//! replaced at runtime — including through the builder while composing the
//! server. Custom protocols get their own handler via [`UnifiedServer::custom_handler`].
//!
//! ## Usage
//!
//! ```rust,ignore
//! use aex::unified::{UnifiedServer, DetectorRegistry};
//!
//! let server = UnifiedServer::new(addr, globals)
//!     .http_handler(my_http_handler)
//!     .tcp_handler(my_tcp_handler)
//!     .udp_handler(my_udp_handler)
//!     .detector(Arc::new(MyTlsDetector))
//!     .custom_handler("my-proto", Arc::new(|ctx| tokio::spawn(async move { /* ... */ })));
//! ```

use bytes::Bytes;
use h2::server;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::os::fd::AsRawFd;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream, UdpSocket};

/// h2 握手输入：直接使用 socket，或合并 peek 阶段已读走的 initial_data。
enum H2Io {
    Owned(TcpStream),
    Combined {
        reader: Option<
            tokio::io::BufReader<
                tokio::io::Chain<std::io::Cursor<Vec<u8>>, tokio::net::tcp::OwnedReadHalf>,
            >,
        >,
        writer: Option<tokio::net::tcp::OwnedWriteHalf>,
    },
}

impl AsyncRead for H2Io {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match &mut *self {
            H2Io::Owned(s) => Pin::new(s).poll_read(cx, buf),
            H2Io::Combined { reader, .. } => {
                let r = reader.as_mut().expect("reader taken");
                Pin::new(r).poll_read(cx, buf)
            }
        }
    }
}

impl AsyncWrite for H2Io {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match &mut *self {
            H2Io::Owned(s) => Pin::new(s).poll_write(cx, buf),
            H2Io::Combined { writer, .. } => {
                let w = writer.as_mut().expect("writer taken");
                Pin::new(w).poll_write(cx, buf)
            }
        }
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
    ) -> Poll<std::io::Result<()>> {
        match &mut *self {
            H2Io::Owned(s) => Pin::new(s).poll_flush(cx),
            H2Io::Combined { writer, .. } => {
                let w = writer.as_mut().expect("writer taken");
                Pin::new(w).poll_flush(cx)
            }
        }
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
    ) -> Poll<std::io::Result<()>> {
        match &mut *self {
            H2Io::Owned(s) => Pin::new(s).poll_shutdown(cx),
            H2Io::Combined { writer, .. } => {
                let w = writer.as_mut().expect("writer taken");
                Pin::new(w).poll_shutdown(cx)
            }
        }
    }
}


pub mod detect;
pub use detect::{
    run_pipeline, Claim, DetectionEvent, DetectionState, DetectorMode, DetectorRegistry,
    Http11Detector, Http2Detector, Position, ProtocolDetector, RegisterError, Verdict, MAX_PEEK,
};

use crate::connection::context::{BoxReader, BoxWriter, ConnectionFd, Context};
use crate::http::meta::HttpMetadata;
use crate::http::middlewares::websocket::WebSocket;
use crate::http::protocol::header::HeaderKey;
use crate::http::protocol::method::HttpMethod;
use crate::http::protocol::version::HttpVersion;
use crate::http::router::Router as HttpRouter;

pub const H2_CONNECTION_PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

pub const HTTP_METHODS: &[&[u8]] = &[
    b"GET ",
    b"POST ",
    b"PUT ",
    b"DELETE ",
    b"PATCH ",
    b"HEAD ",
    b"OPTIONS ",
    b"CONNECT ",
    b"TRACE ",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Http11,
    Http2,
    TCP,
    UDP,
    Unknown,
}

impl Protocol {
    /// Legacy prefix-based classification kept for compatibility; new code
    /// should use the pluggable [`DetectorRegistry`] pipeline instead.
    pub fn detect(bytes: &[u8], is_udp: bool) -> Self {
        if is_udp {
            return Protocol::UDP;
        }

        if bytes.is_empty() {
            return Protocol::Unknown;
        }

        if bytes.starts_with(H2_CONNECTION_PREFACE) {
            return Protocol::Http2;
        }

        for method in HTTP_METHODS {
            if bytes.starts_with(method) {
                return Protocol::Http11;
            }
        }

        Protocol::TCP
    }
}

pub type HttpHandler =
    Arc<dyn Fn(&mut Context) -> futures::future::BoxFuture<'_, bool> + Send + Sync>;
pub type Http2Handler =
    Arc<dyn Fn(&mut Context) -> futures::future::BoxFuture<'static, bool> + Send + Sync>;
pub type TCPHandler = Arc<dyn Fn(Context) -> tokio::task::JoinHandle<()> + Send + Sync>;
pub type UDPHandler = Arc<dyn Fn(Context) -> tokio::task::JoinHandle<()> + Send + Sync>;

pub struct UnifiedServer {
    pub addr: SocketAddr,
    pub globals: Arc<crate::connection::global::GlobalContext>,
    pub http_router: Option<Arc<HttpRouter>>,
    pub http_handler: Option<HttpHandler>,
    pub enable_http2: bool,
    pub http2_handler: Option<Http2Handler>,
    pub tcp_handler: Option<TCPHandler>,
    pub udp_handler: Option<UDPHandler>,
    /// Ordered protocol detectors consulted before dispatching a TCP
    /// connection. Shared via `Arc`, so runtime mutations are visible to the
    /// running server.
    pub registry: Arc<DetectorRegistry>,
    /// Handlers for protocols claimed by custom detectors, keyed by the
    /// detector's `protocol()` label.
    pub custom_handlers: HashMap<String, TCPHandler>,
    /// Master switch for the detection phase; when off, connections go
    /// straight to the TCP handler without any peeking.
    pub detect_enabled: bool,
    #[cfg(feature = "proxy")]
    /// Serve absolute-form requests and CONNECT tunnels as an HTTP forward
    /// proxy on the same port as website traffic.
    pub http_proxy_enabled: bool,
    #[cfg(feature = "proxy")]
    /// Claim "socks" greetings internally and serve SOCKS4/4a/5 CONNECT.
    pub socks_proxy_enabled: bool,
    #[cfg(feature = "proxy")]
    /// Shared credential checker for both proxy services.
    pub proxy_authorizer: Option<crate::proxy::ProxyAuthorizer>,
    #[doc(hidden)]
    pub _udp_socket: Option<UdpSocket>,
}

impl UnifiedServer {
    pub fn new(addr: SocketAddr, globals: Arc<crate::connection::global::GlobalContext>) -> Self {
        Self {
            addr,
            globals,
            http_router: None,
            http_handler: None,
            enable_http2: false,
            http2_handler: None,
            tcp_handler: None,
            udp_handler: None,
            registry: Arc::new(DetectorRegistry::new()),
            custom_handlers: HashMap::new(),
            detect_enabled: true,
            #[cfg(feature = "proxy")]
            http_proxy_enabled: false,
            #[cfg(feature = "proxy")]
            socks_proxy_enabled: false,
            #[cfg(feature = "proxy")]
            proxy_authorizer: None,
            _udp_socket: None,
        }
    }

    pub fn http_router(mut self, router: HttpRouter) -> Self {
        self.http_router = Some(Arc::new(router));
        self
    }

    pub fn http_handler(mut self, handler: HttpHandler) -> Self {
        self.http_handler = Some(handler);
        self
    }

    pub fn enable_http2(mut self) -> Self {
        self.enable_http2 = true;
        self
    }

    pub fn http2_handler(mut self, handler: Http2Handler) -> Self {
        self.http2_handler = Some(handler);
        self
    }

    pub fn tcp_handler(mut self, handler: TCPHandler) -> Self {
        self.tcp_handler = Some(handler);
        self
    }

    pub fn udp_handler(mut self, handler: UDPHandler) -> Self {
        self.udp_handler = Some(handler);
        self
    }

    /// Register a protocol detector at the back of the detection pipeline.
    /// Registration errors (duplicate name, conflict) are logged, not fatal.
    pub fn detector(self, d: Arc<dyn ProtocolDetector>) -> Self {
        self.detector_at(Position::Back, d)
    }

    /// Register a protocol detector at an explicit position.
    pub fn detector_at(self, pos: Position, d: Arc<dyn ProtocolDetector>) -> Self {
        if let Err(e) = self.registry.register_at(pos, d) {
            tracing::warn!("[Unified] detector registration failed: {}", e);
        }
        self
    }

    /// Share an externally-managed registry, e.g. one mutated at runtime by
    /// other parts of the application.
    pub fn with_registry(mut self, registry: Arc<DetectorRegistry>) -> Self {
        self.registry = registry;
        self
    }

    /// Route connections claimed as `protocol` to a dedicated handler.
    pub fn custom_handler<P: Into<String>>(mut self, protocol: P, handler: TCPHandler) -> Self {
        self.custom_handlers.insert(protocol.into(), handler);
        self
    }

    /// Enable/disable the detection phase entirely. When disabled, TCP
    /// connections go straight to the TCP handler without peeking.
    pub fn detection(mut self, enabled: bool) -> Self {
        self.detect_enabled = enabled;
        self
    }

    #[cfg(feature = "proxy")]
    /// Serve HTTP forward-proxy traffic (absolute-form requests and CONNECT
    /// tunnels) on this server. Website traffic is unaffected — the client's
    /// request line decides which service handles each connection.
    pub fn enable_http_proxy(mut self) -> Self {
        // Proxy traffic arrives as ordinary HTTP/1.x — without this detector
        // nothing claims the connection and the proxy hook never runs.
        if let Err(e) = self
            .registry
            .register_at(Position::Back, Arc::new(Http11Detector))
        {
            tracing::warn!("[Unified] http11 detector registration failed: {}", e);
        }
        self.http_proxy_enabled = true;
        self
    }

    #[cfg(feature = "proxy")]
    /// Serve SOCKS4/4a/5 CONNECT on this server. Registers the internal
    /// SOCKS greeting detector; claimed connections are handled before any
    /// user `custom_handler("socks")`.
    pub fn enable_socks_proxy(mut self) -> Self {
        use crate::proxy::{SocksDetector, socks_tcp_handler};
        if let Err(e) = self.registry.register(Arc::new(SocksDetector)) {
            tracing::warn!("[Unified] socks detector registration failed: {}", e);
        }
        self.socks_proxy_enabled = true;
        // Pre-seed the internal handler; users may still override it via
        // custom_handler("socks", ...) for full control.
        self.custom_handlers
            .entry("socks".to_string())
            .or_insert_with(|| {
                socks_tcp_handler(self.proxy_authorizer.clone())
            });
        self
    }

    #[cfg(feature = "proxy")]
    /// Credential gate shared by the HTTP and SOCKS proxy services.
    pub fn proxy_authenticator(
        mut self,
        f: Arc<dyn Fn(&str, &str) -> bool + Send + Sync>,
    ) -> Self {
        self.proxy_authorizer = Some(f);
        // Refresh a pre-existing socks handler so it sees the authenticator.
        if self.socks_proxy_enabled {
            self.custom_handlers.insert(
                "socks".to_string(),
                crate::proxy::socks_tcp_handler(self.proxy_authorizer.clone()),
            );
        }
        self
    }

    #[cfg(feature = "proxy")]
    /// Convenience: enable both proxy services.
    pub fn enable_proxies(self) -> Self {
        self.enable_http_proxy().enable_socks_proxy()
    }

    pub async fn handle_tcp_connection(&self, mut socket: TcpStream, peer_addr: SocketAddr) {
        // Detection phase: buffer bytes and run the detector pipeline until
        // some detector claims the connection, every pending detector has
        // passed, or the peek cap is reached. With an empty registry (or
        // detection disabled) nothing is read here at all.
        let detectors = self.registry.snapshot();
        let mut initial_data: Vec<u8> = Vec::with_capacity(256);
        let mut state = DetectionState::new();

        if self.detect_enabled && !detectors.is_empty() {
            let mut chunk = [0u8; 2048];
            loop {
                match socket.read(&mut chunk).await {
                    Ok(0) | Err(_) => {
                        if initial_data.is_empty() {
                            return;
                        }
                        state.finish();
                        break;
                    }
                    Ok(n) => initial_data.extend_from_slice(&chunk[..n]),
                }
                run_pipeline(&detectors, &initial_data, &mut state);
                if state.is_finished() || initial_data.len() >= MAX_PEEK {
                    break;
                }
            }
        }

        let claim = state.claim();
        match claim.map(|c| (c.protocol.as_str(), c.mode)) {
            Some(("http", DetectorMode::Standard)) => {
                self.handle_http11(socket, peer_addr, initial_data).await;
            }
            Some(("http2", DetectorMode::Standard)) if self.enable_http2 => {
                self.handle_http2(socket, peer_addr, initial_data).await;
            }
            Some((protocol, mode)) => {
                let handler = self.custom_handlers.get(protocol).cloned().or_else(|| {
                    tracing::warn!(
                        "[Unified] no handler for detected protocol `{protocol}` ({mode:?}), falling back to TCP"
                    );
                    self.tcp_handler.clone()
                });
                match handler {
                    Some(h) => {
                        self.dispatch_tcp(socket, peer_addr, initial_data, state, h)
                            .await;
                    }
                    None => {
                        tracing::warn!(
                            "[Unified] No TCP handler registered, dropping connection from {}",
                            peer_addr
                        );
                    }
                }
            }
            None => {
                self.handle_tcp(socket, peer_addr, initial_data, Some(state))
                    .await;
            }
        }
    }

    /// Install the split socket halves plus link-state attributes into a
    /// fresh context and hand it to `handler`.
    async fn dispatch_tcp(
        &self,
        socket: TcpStream,
        peer_addr: SocketAddr,
        initial_data: Vec<u8>,
        state: DetectionState,
        handler: TCPHandler,
    ) {
        let fd = socket.as_raw_fd();
        let (reader, writer) = socket.into_split();
        let cursor = std::io::Cursor::new(initial_data);
        let reader_with_buf = tokio::io::BufReader::new(cursor.chain(reader));
        let boxed_reader: BoxReader = Box::new(reader_with_buf);
        let writer = Box::new(writer) as BoxWriter;

        let mut ctx = Context::new(
            Some(boxed_reader),
            Some(writer),
            self.globals.clone(),
            peer_addr,
        );
        ctx.local.set_value(ConnectionFd(fd));
        ctx.local.set_value(state);
        handler(ctx);
    }

    async fn handle_http11(
        &self,
        socket: TcpStream,
        peer_addr: SocketAddr,
        initial_bytes: Vec<u8>,
    ) {
        let local_addr = socket.local_addr().ok();
        let (reader, writer) = socket.into_split();
        let cursor = std::io::Cursor::new(initial_bytes);
        let reader_with_buf = tokio::io::BufReader::new(cursor.chain(reader));
        let boxed_reader: BoxReader = Box::new(reader_with_buf);
        let writer = Box::new(tokio::io::BufWriter::new(writer)) as BoxWriter;

        let mut ctx = Context::new(
            Some(boxed_reader),
            Some(writer),
            self.globals.clone(),
            peer_addr,
        );
        ctx.local_addr = local_addr;

        if ctx.req().parse_to_local().await.is_err() {
            let _ = ctx.res().send_failure().await;
            return;
        }

        #[cfg(feature = "proxy")]
        if self.http_proxy_enabled
            && crate::proxy::maybe_handle_http_proxy(&mut ctx, self.proxy_authorizer.as_ref())
                .await
        {
            return; // connection fully served as proxy traffic
        }

        let is_ws = {
            let meta = ctx.local.get_ref::<HttpMetadata>();
            meta.map(|m| m.is_websocket).unwrap_or(false)
        };

        let handled = if is_ws {
            if let Some(router) = &self.http_router {
                router.on_request(&mut ctx).await
            } else {
                false
            }
        } else if let Some(handler) = &self.http_handler {
            handler(&mut ctx).await
        } else if let Some(router) = &self.http_router {
            router.on_request(&mut ctx).await
        } else {
            false
        };

        if handled {
            let _ = ctx.res().send_response().await;
        } else {
            let _ = ctx.res().send_failure().await;
        }
    }

    async fn handle_http2(&self, socket: TcpStream, peer_addr: SocketAddr, initial_data: Vec<u8>) {
        let conn_stream = if initial_data.is_empty() {
            // 无 peek 数据，直接用原始 socket
            H2Io::Owned(socket)
        } else {
            // Peek 阶段已读走 preface，需与 socket 拼回才能完成 h2 handshake
            let (reader, writer) = socket.into_split();
            H2Io::Combined {
                reader: Some(tokio::io::BufReader::new(
                    std::io::Cursor::new(initial_data).chain(reader),
                )),
                writer: Some(writer),
            }
        };
        let mut conn = match server::handshake(conn_stream).await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("[H2] handshake failed: {}", e);
                return;
            }
        };

        let globals = self.globals.clone();
        let handler = self.http2_handler.clone();

        loop {
            tokio::select! {
                frame = conn.accept() => {
                    match frame {
                        Some(Ok((request, mut responder))) => {
                            let path = request.uri().path().to_string();
                            let method_str = request.method().as_str();
                            let http_method = HttpMethod::from_str(method_str).unwrap_or(HttpMethod::GET);

                            let mut meta = HttpMetadata::default();
                            meta.method = http_method;
                            meta.path = path.clone();
                            meta.version = HttpVersion::Http20;

                            for (name, value) in request.headers() {
                                if let Some(header_key) = HeaderKey::from_str(name.as_str()) {
                                    if let Ok(val) = value.to_str() {
                                        meta.headers.insert(header_key, val.to_string());
                                    }
                                }
                            }

                            let is_ws = WebSocket::check(http_method, &meta.headers);
                            if is_ws {
                                meta.is_websocket = true;
                            }

                            let mut ctx = Context::new(None, None, globals.clone(), peer_addr);
                            ctx.set(meta);

                            if let Some(h) = &handler {
                                h(&mut ctx).await;
                            } else {
                                tracing::warn!("[H2] No HTTP/2 handler registered");
                            }

                            let meta = ctx.local.get_ref::<HttpMetadata>();
                            let status = if let Some(m) = meta {
                                m.status.to_http_status()
                            } else {
                                http::StatusCode::OK
                            };
                            let mut body_str = String::new();
                            if let Some(m) = meta {
                                body_str = String::from_utf8_lossy(&m.body).to_string();
                            }

                            let resp_builder = http::Response::builder().status(status);

                            match resp_builder.body(()) {
                                Ok(resp) => {
                                    if let Ok(mut send_stream) = responder.send_response(resp, false) {
                                        let _ = send_stream.send_data(Bytes::from(body_str), true);
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!("[H2] build response failed: {}", e);
                                }
                            }
                        }
                        Some(Err(e)) => {
                            tracing::warn!("[H2] frame error: {}", e);
                        }
                        None => break,
                    }
                }
            }
        }
    }

    async fn handle_tcp(
        &self,
        socket: TcpStream,
        peer_addr: SocketAddr,
        initial_data: Vec<u8>,
        detection: Option<DetectionState>,
    ) {
        let fd = socket.as_raw_fd();
        let local_addr = socket.local_addr().ok();
        let (reader, writer) = socket.into_split();
        let cursor = std::io::Cursor::new(initial_data);
        let reader_with_buf = tokio::io::BufReader::new(cursor.chain(reader));
        let boxed_reader: BoxReader = Box::new(reader_with_buf);
        let writer = Box::new(writer) as BoxWriter;

        let mut ctx = Context::new(
            Some(boxed_reader),
            Some(writer),
            self.globals.clone(),
            peer_addr,
        );
        ctx.local_addr = local_addr;
        ctx.local.set_value(ConnectionFd(fd));
        if let Some(state) = detection {
            ctx.local.set_value(state);
        }

        tracing::info!(
            "[Unified] TCP handler invoked for connection from {}",
            peer_addr
        );

        if let Some(handler) = &self.tcp_handler {
            handler(ctx);
        } else {
            tracing::warn!(
                "[Unified] No TCP handler registered, dropping connection from {}",
                peer_addr
            );
        }
    }

    pub async fn start(&self) -> anyhow::Result<()> {
        let tcp_listener = TcpListener::bind(self.addr).await?;
        tracing::info!("[Unified] TCP listening on {}", self.addr);

        let self_arc = Arc::new(self.clone());

        if let Some(udp_handler) = &self_arc.udp_handler {
            let sock = Arc::new(UdpSocket::bind(self_arc.addr).await?);
            tracing::info!("[Unified] UDP listening on {}", sock.local_addr()?);

            let handler = udp_handler.clone();
            let globals = self_arc.globals.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 65535];
                loop {
                    match sock.recv_from(&mut buf).await {
                        Ok((n, peer)) => {
                            let data = buf[..n].to_vec();
                            let mut ctx = Context::new(None, None, globals.clone(), peer);
                            ctx.set(data);
                            let handler = handler.clone();
                            handler(ctx);
                        }
                        Err(e) => {
                            tracing::warn!("[Unified] UDP recv error: {}", e);
                            break;
                        }
                    }
                }
            });
        }

        loop {
            tokio::select! {
                result = tcp_listener.accept() => {
                    match result {
                        Ok((socket, peer_addr)) => {
                            let srv = self_arc.clone();
                            tokio::spawn(async move {
                                srv.handle_tcp_connection(socket, peer_addr).await;
                            });
                        }
                        Err(e) => {
                            tracing::warn!("[Unified] Accept error: {}", e);
                        }
                    }
                }
            }
        }
    }

    pub async fn start_tcp<F, C>(&self) -> anyhow::Result<()>
    where
        F: crate::tcp::types::TCPFrame + Send + Sync + 'static,
        C: crate::tcp::types::TCPCommand + Send + Sync + 'static,
    {
        let tcp_listener = TcpListener::bind(self.addr).await?;
        tracing::info!("[Unified] TCP listening on {}", self.addr);

        let globals = self.globals.clone();
        let tcp_handler = self.tcp_handler.clone();

        loop {
            match tcp_listener.accept().await {
                Ok((socket, peer_addr)) => {
                    let handler = tcp_handler.clone();
                    let globals = globals.clone();
                    tokio::spawn(async move {
                        let fd = socket.as_raw_fd();
                        let local_addr = socket.local_addr().ok();
                        let (reader, writer) = socket.into_split();
                        let reader = tokio::io::BufReader::new(reader);
                        let boxed_reader: BoxReader = Box::new(reader);
                        let writer = Box::new(writer) as BoxWriter;

                        let mut ctx =
                            Context::new(Some(boxed_reader), Some(writer), globals, peer_addr);
                        ctx.local_addr = local_addr;
                        ctx.local.set_value(ConnectionFd(fd));
                        if let Some(h) = handler {
                            h(ctx);
                        }
                    });
                }
                Err(e) => {
                    tracing::warn!("[Unified] Accept error: {}", e);
                }
            }
        }
    }

    pub async fn start_udp<F, C>(&self) -> anyhow::Result<()>
    where
        F: crate::tcp::types::Frame + Send + Sync + Clone + 'static,
        C: crate::tcp::types::Command + Send + Sync + 'static,
    {
        let sock = Arc::new(UdpSocket::bind(self.addr).await?);
        tracing::info!("[Unified] UDP listening on {}", sock.local_addr()?);

        let globals = self.globals.clone();
        let udp_handler = self.udp_handler.clone();

        let mut buf = [0u8; 65535];
        loop {
            match sock.recv_from(&mut buf).await {
                Ok((n, peer)) => {
                    let data = buf[..n].to_vec();
                    let handler = udp_handler.clone();
                    let globals = globals.clone();
                    tokio::spawn(async move {
                        let mut ctx = Context::new(None, None, globals, peer);
                        ctx.set(data);
                        if let Some(h) = handler {
                            h(ctx);
                        }
                    });
                }
                Err(e) => {
                    tracing::warn!("[Unified] UDP recv error: {}", e);
                }
            }
        }
    }
}

impl Clone for UnifiedServer {
    fn clone(&self) -> Self {
        Self {
            addr: self.addr,
            globals: self.globals.clone(),
            http_router: self.http_router.clone(),
            http_handler: self.http_handler.clone(),
            enable_http2: self.enable_http2,
            http2_handler: self.http2_handler.clone(),
            tcp_handler: self.tcp_handler.clone(),
            udp_handler: self.udp_handler.clone(),
            registry: self.registry.clone(),
            custom_handlers: self.custom_handlers.clone(),
            detect_enabled: self.detect_enabled,
            #[cfg(feature = "proxy")]
            http_proxy_enabled: self.http_proxy_enabled,
            #[cfg(feature = "proxy")]
            socks_proxy_enabled: self.socks_proxy_enabled,
            #[cfg(feature = "proxy")]
            proxy_authorizer: self.proxy_authorizer.clone(),
            _udp_socket: None,
        }
    }
}
