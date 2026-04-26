//! # Unified Protocol Server
//!
//! Unified server supporting HTTP/1.1, HTTP/2, WebSocket, TCP, and UDP protocols on the same port.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use aex::unified::{UnifiedServer, Protocol};
//! use aex::http::router::Router as HttpRouter;
//! use aex::exe;
//!
//! let server = UnifiedServer::new(addr, globals)
//!     .http_handler(my_http_handler)
//!     .tcp_handler(my_tcp_handler)
//!     .udp_handler(my_udp_handler);
//! ```

use std::any::TypeId;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use bytes::Bytes;
use h2::server;

use crate::connection::context::{Context, BoxReader, BoxWriter};
use crate::http::meta::HttpMetadata;
use crate::http::middlewares::websocket::WebSocket;
use crate::http::protocol::header::HeaderKey;
use crate::http::protocol::method::HttpMethod;
use crate::http::protocol::version::HttpVersion;
use crate::http::router::Router as HttpRouter;

pub const H2_CONNECTION_PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

pub const HTTP_METHODS: &[&[u8]] = &[
    b"GET ", b"POST ", b"PUT ", b"DELETE ", b"PATCH ",
    b"HEAD ", b"OPTIONS ", b"CONNECT ", b"TRACE ",
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

pub type HttpHandler = Arc<dyn Fn(&mut Context) -> futures::future::BoxFuture<'_, bool> + Send + Sync>;
pub type Http2Handler = Arc<dyn Fn(&mut Context) -> futures::future::BoxFuture<'static, bool> + Send + Sync>;
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

    pub async fn handle_tcp_connection(&self, mut socket: TcpStream, peer_addr: SocketAddr) {
        let mut peek_buf = [0u8; 24];
        let n = match socket.read(&mut peek_buf).await {
            Ok(n) => n,
            Err(_) => return,
        };
        if n == 0 { return; }

        let protocol = Protocol::detect(&peek_buf[..n], false);
        let initial_data = peek_buf[..n].to_vec();

        match protocol {
            Protocol::Http2 => {
                if self.enable_http2 {
                    self.handle_http2(socket, peer_addr).await;
                } else {
                    self.handle_tcp(socket, peer_addr).await;
                }
            }
            Protocol::Http11 => {
                self.handle_http11(socket, peer_addr, initial_data).await;
            }
            Protocol::TCP | Protocol::Unknown => {
                self.handle_tcp(socket, peer_addr).await;
            }
            Protocol::UDP => {
                self.handle_tcp(socket, peer_addr).await;
            }
        }
    }

    async fn handle_http11(&self, socket: TcpStream, peer_addr: SocketAddr, initial_bytes: Vec<u8>) {
        let (reader, writer) = socket.into_split();
        let cursor = std::io::Cursor::new(initial_bytes);
        let reader_with_buf = tokio::io::BufReader::new(cursor.chain(reader));
        let boxed_reader: BoxReader = Box::new(reader_with_buf);
        let writer = Box::new(tokio::io::BufWriter::new(writer)) as BoxWriter;

        let mut ctx = Context::new(Some(boxed_reader), Some(writer), self.globals.clone(), peer_addr);

        if ctx.req().parse_to_local().await.is_err() {
            let _ = ctx.res().send_failure().await;
            return;
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

    async fn handle_http2(&self, socket: TcpStream, peer_addr: SocketAddr) {
        let mut conn = match server::handshake(socket).await {
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
                            let mut resp_headers: crate::http::protocol::header::Headers = crate::http::protocol::header::Headers::new();
                            if let Some(m) = meta {
                                body_str = String::from_utf8_lossy(&m.body).to_string();
                                resp_headers = m.headers.clone();
                            }

                            let mut resp_builder = http::Response::builder().status(status);

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

    async fn handle_tcp(&self, socket: TcpStream, peer_addr: SocketAddr) {
        let (reader, writer) = socket.into_split();
        let reader = tokio::io::BufReader::new(reader);
        let boxed_reader: BoxReader = Box::new(reader);
        let writer = Box::new(writer) as BoxWriter;

        let mut ctx = Context::new(Some(boxed_reader), Some(writer), self.globals.clone(), peer_addr);

        if let Some(handler) = &self.tcp_handler {
            handler(ctx);
        } else {
            tracing::warn!("[Unified] No TCP handler registered, dropping connection from {}", peer_addr);
        }
    }

    pub async fn start(&self) -> anyhow::Result<()> {
        let tcp_listener = TcpListener::bind(self.addr).await?;
        tracing::info!("[Unified] TCP listening on {}", self.addr);

        if let Some(udp_handler) = &self.udp_handler {
            let sock = Arc::new(UdpSocket::bind(self.addr).await?);
            tracing::info!("[Unified] UDP listening on {}", sock.local_addr()?);

            let handler = udp_handler.clone();
            let globals = self.globals.clone();
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

        let server = self.clone();

        loop {
            tokio::select! {
                result = tcp_listener.accept() => {
                    match result {
                        Ok((socket, peer_addr)) => {
                            let srv = server.clone();
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
                        let (reader, writer) = socket.into_split();
                        let reader = tokio::io::BufReader::new(reader);
                        let boxed_reader: BoxReader = Box::new(reader);
                        let writer = Box::new(writer) as BoxWriter;

                        let mut ctx = Context::new(Some(boxed_reader), Some(writer), globals, peer_addr);
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
                    let sock = sock.clone();
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
        
        Ok(())
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
            _udp_socket: None,
        }
    }
}