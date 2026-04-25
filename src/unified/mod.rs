//! # Unified Protocol Server
//!
//! Unified server supporting HTTP/1.1, HTTP/2, WebSocket, and P2P protocols on the same port.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use aex::unified::{UnifiedServer, Protocol};
//! use aex::http::router::Router as HttpRouter;
//! use aex::exe;
//!
//! let mut http_router = HttpRouter::new(NodeType::Static("root".into()));
//! http_router.get("/", exe!(|ctx| {
//!     ctx.send("Hello!");
//!     true
//! })).register();
//!
//! let server = UnifiedServer::new(addr, globals)
//!     .http_router(http_router)
//!     .enable_http2()
//!     .p2p_handler(Arc::new(|socket, addr| {
//!         tokio::spawn(handle_p2p_connection(socket, addr))
//!     }));
//! ```

use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt};
use tokio::net::TcpStream;
use bytes::Bytes;
use h2::server;

use crate::connection::context::{Context, BoxReader, BoxWriter};
use crate::connection::entry::ConnectionEntry;
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
    P2P,
    Unknown,
}

impl Protocol {
    pub fn detect(bytes: &[u8]) -> Self {
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

        Protocol::P2P
    }
}

pub type P2PHandler = Arc<dyn Fn(TcpStream, SocketAddr) + Send + Sync>;

pub struct UnifiedServer {
    pub addr: SocketAddr,
    pub globals: Arc<crate::connection::global::GlobalContext>,
    pub http_router: Option<Arc<HttpRouter>>,
    pub enable_http2: bool,
    pub p2p_handler: Option<P2PHandler>,
}

impl UnifiedServer {
    pub fn new(addr: SocketAddr, globals: Arc<crate::connection::global::GlobalContext>) -> Self {
        Self {
            addr,
            globals,
            http_router: None,
            enable_http2: false,
            p2p_handler: None,
        }
    }

    pub fn http_router(mut self, router: HttpRouter) -> Self {
        self.http_router = Some(Arc::new(router));
        self
    }

    pub fn enable_http2(mut self) -> Self {
        self.enable_http2 = true;
        self
    }

    pub fn p2p_handler(mut self, handler: P2PHandler) -> Self {
        self.p2p_handler = Some(handler);
        self
    }

    pub async fn handle_connection(&self, mut socket: TcpStream, peer_addr: SocketAddr) {
        let mut peek_buf = [0u8; 24];
        let n = match socket.read(&mut peek_buf).await {
            Ok(n) => n,
            Err(_) => return,
        };
        if n == 0 { return; }

        let protocol = Protocol::detect(&peek_buf[..n]);
        let initial_data = peek_buf[..n].to_vec();

        match protocol {
            Protocol::Http2 => {
                if self.enable_http2 {
                    self.handle_http2(socket, peer_addr).await;
                } else {
                    self.handle_p2p(socket, peer_addr).await;
                }
            }
            Protocol::Http11 => {
                self.handle_http11(socket, peer_addr, initial_data).await;
            }
            Protocol::P2P | Protocol::Unknown => {
                self.handle_p2p(socket, peer_addr).await;
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

        let router = match &self.http_router {
            Some(r) => r.clone(),
            None => {
                let _ = ctx.res().send_failure().await;
                return;
            }
        };

        let is_ws = {
            let meta = ctx.local.get_ref::<HttpMetadata>();
            meta.map(|m| m.is_websocket).unwrap_or(false)
        };

        if is_ws {
            tracing::debug!("[WS] Upgrade request for {}", peer_addr);
        }

        if router.on_request(&mut ctx).await {
            let _ = ctx.res().send_response().await;
        } else {
            let _ = ctx.res().send_failure().await;
        }
    }

    async fn handle_http2(&self, socket: TcpStream, peer_addr: SocketAddr) {
        tracing::info!("[H2] Connection from {}", peer_addr);

        let mut conn = match server::handshake(socket).await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("[H2] handshake failed: {}", e);
                return;
            }
        };

        let router = match &self.http_router {
            Some(r) => r.clone(),
            None => return,
        };
        let globals = self.globals.clone();

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

                            let _route_found = router.on_request(&mut ctx).await;

                            let (status, body, headers) = {
                                let meta = ctx.local.get_ref::<HttpMetadata>().unwrap();
                                (meta.status.to_http_status(), meta.body.clone(), meta.headers.clone())
                            };

                            let mut resp_builder = http::Response::builder().status(status);
                            for (key, val) in headers.iter() {
                                if let Ok(h2_name) = http::header::HeaderName::from_bytes(key.as_str().as_bytes()) {
                                    resp_builder = resp_builder.header(h2_name, val.as_str());
                                }
                            }

                            match resp_builder.body(()) {
                                Ok(resp) => {
                                    if let Ok(mut send_stream) = responder.send_response(resp, false) {
                                        let _ = send_stream.send_data(Bytes::from(body), true);
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

    async fn handle_p2p(&self, socket: TcpStream, peer_addr: SocketAddr) {
        if let Some(handler) = &self.p2p_handler {
            handler(socket, peer_addr);
        } else {
            tracing::warn!("[P2P] No handler registered, dropping connection from {}", peer_addr);
        }
    }

    pub async fn start(&self) -> anyhow::Result<()> {
        let listener = tokio::net::TcpListener::bind(self.addr).await?;
        tracing::info!("[Unified] Server listening on {}", self.addr);

        loop {
            match listener.accept().await {
                Ok((socket, peer_addr)) => {
                    let server = self.clone();
                    tokio::spawn(async move {
                        server.handle_connection(socket, peer_addr).await;
                    });
                }
                Err(e) => {
                    tracing::warn!("[Unified] Accept error: {}", e);
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
            enable_http2: self.enable_http2,
            p2p_handler: self.p2p_handler.clone(),
        }
    }
}