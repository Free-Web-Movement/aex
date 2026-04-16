//! # Server
//!
//! Unified multi-protocol server supporting HTTP, TCP, and UDP.
//!
//! ## Example
//!
//! ```rust,ignore
//! use aex::http::router::{NodeType, Router as HttpRouter};
//! use aex::server::HTTPServer;
//! use aex::tcp::types::RawCodec;
//! use aex::exe;
//! use std::net::SocketAddr;
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let addr: SocketAddr = "0.0.0.0:8080".parse()?;
//!     let mut router = HttpRouter::new(NodeType::Static("root".into()));
//!
//!     router.get("/", exe!(|ctx| {
//!         ctx.send("Hello!");
//!         true
//!     })).register();
//!
//!     HTTPServer::new(addr, None)
//!         .http(router)
//!         .start()
//!         .await?;
//!     Ok(())
//! }
//! ```

use crate::connection::context::TypeMapExt;
use crate::connection::entry::ConnectionEntry;
use crate::connection::global::GlobalContext;
use crate::connection::types::IDExtractor;
use crate::crypto::session_key_manager::PairedSessionKey;
use crate::http::router::Router as HttpRouter;
use crate::tcp::router::Router as TcpRouter;
use crate::tcp::types::{TCPCommand, TCPFrame, RawCodec};
use crate::udp::router::Router as UdpRouter;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

type Extractor = Arc<dyn Fn(&RawCodec) -> u32 + Send + Sync>;

/// Multi-protocol server supporting HTTP, TCP, and UDP.
///
/// # Example
///
/// ```rust,ignore
/// Server::new(addr, None)
///     .http(http_router)
///     .tcp(tcp_router, Arc::new(|c| c.id()))
///     .udp(udp_router, Arc::new(|c| c.id()))
///     .start()
///     .await?;
/// ```
#[derive(Clone)]
pub struct Server {
    pub addr: SocketAddr,
    pub globals: Arc<GlobalContext>,
    tcp_extractor: Option<Extractor>,
    udp_extractor: Option<Extractor>,
}

impl Server {
    /// Creates a new Server instance.
    pub fn new(addr: SocketAddr, globals: Option<Arc<GlobalContext>>) -> Self {
        Self {
            addr,
            globals: globals.unwrap_or(Arc::new(GlobalContext::new(
                addr,
                Some(Arc::new(Mutex::new(PairedSessionKey::new(16)))),
            ))),
            tcp_extractor: None,
            udp_extractor: None,
        }
    }

    /// Sets the HTTP router (HTTP/1.1).
    pub fn http(mut self, router: HttpRouter) -> Self {
        self.globals.routers.set_value(Arc::new(router));
        self
    }

    /// Enables HTTP/2 support using the same router as HTTP/1.1.
    pub fn http2(mut self) -> Self {
        let global = self.globals.clone();
        if let Some(http_router) = global.routers.get_value::<Arc<HttpRouter>>() {
            let h2_codec = Arc::new(crate::http2::H2Codec::new(
                http_router,
                global,
            ));
            *self.globals.h2_codec.write().unwrap() = Some(h2_codec);
        }
        self
    }

    /// Sets the TCP router with extractor.
    pub fn tcp<C: 'static>(mut self, router: TcpRouter, extractor: Arc<dyn Fn(&C) -> u32 + Send + Sync>) -> Self {
        self.globals.routers.insert(
            std::any::TypeId::of::<crate::connection::context::TcpRouterKey>(),
            Box::new(Arc::new(router)),
        );
        self.tcp_extractor = Some(Arc::new(move |c: &RawCodec| {
            let any = c as &dyn std::any::Any;
            if let Some(c) = any.downcast_ref::<C>() {
                extractor(c)
            } else {
                0
            }
        }));
        self
    }

    /// Sets the UDP router with extractor.
    pub fn udp<C: 'static>(mut self, router: UdpRouter, extractor: Arc<dyn Fn(&C) -> u32 + Send + Sync>) -> Self {
        self.globals.routers.insert(
            std::any::TypeId::of::<crate::connection::context::UdpRouterKey>(),
            Box::new(Arc::new(router)),
        );
        self.udp_extractor = Some(Arc::new(move |c: &RawCodec| {
            let any = c as &dyn std::any::Any;
            if let Some(c) = any.downcast_ref::<C>() {
                extractor(c)
            } else {
                0
            }
        }));
        self
    }

    /// Starts the server with all configured protocols.
    pub async fn start(&self) -> anyhow::Result<()> {
        let extractor = self.tcp_extractor.clone().unwrap_or_else(|| Arc::new(|_: &RawCodec| 0));
        let server = Arc::new(self.clone());

        // --- UDP ---
        if self.udp_extractor.is_some() {
            let udp_token = CancellationToken::new();
            let udp_loop_token = udp_token.clone();
            let server_udp = server.clone();
            let extractor_udp = self.udp_extractor.clone().unwrap();

            let udp_handle = tokio::spawn(async move {
                let _ = server_udp.start_udp(extractor_udp, udp_loop_token).await;
            });
            server.globals.add_exit("udp", udp_token, udp_handle.abort_handle()).await;
        }

        // --- TCP ---
        if self.tcp_extractor.is_some() {
            let tcp_token = CancellationToken::new();
            let tcp_loop_token = tcp_token.clone();
            let server_tcp = server.clone();

            let tcp_handle = tokio::spawn(async move {
                let _ = server_tcp.start_tcp(extractor, tcp_loop_token).await;
            });
            server.globals.add_exit("tcp", tcp_token, tcp_handle.abort_handle()).await;
        }

        // --- HTTP (if no TCP/UDP) ---
        if self.tcp_extractor.is_none() && self.udp_extractor.is_none() {
            let http_router = server.globals.routers.get_value::<Arc<HttpRouter>>();
            if let Some(router) = http_router {
                let router = router.clone();
                let globals = server.globals.clone();
                
                tokio::spawn(async move {
                    let listener = match TcpListener::bind(globals.addr).await {
                        Ok(l) => l,
                        Err(e) => {
                            tracing::error!("HTTP bind failed: {}", e);
                            return;
                        }
                    };
                    tracing::info!("HTTP listener started on {}", globals.addr);

                    loop {
                        match listener.accept().await {
                            Ok((socket, peer_addr)) => {
                                let router = router.clone();
                                let globals = globals.clone();
                                tokio::spawn(async move {
                                    use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
                                    
                                    let (reader, writer) = socket.into_split();
                                    let reader = Box::new(BufReader::new(reader)) as Box<dyn tokio::io::AsyncBufRead + Send + Sync + Unpin>;
                                    let writer = Box::new(BufWriter::new(writer)) as Box<dyn tokio::io::AsyncWrite + Send + Sync + Unpin>;
                                    
                                    let mut ctx = crate::connection::context::Context::new(
                                        Some(reader), Some(writer), globals, peer_addr,
                                    );
                                    
                                    if ctx.req().parse_to_local().await.is_ok() {
                                        if router.on_request(&mut ctx).await {
                                            let _ = ctx.res().send_response().await;
                                        } else {
                                            let _ = ctx.res().send_failure().await;
                                        }
                                    }
                                });
                            }
                            Err(e) => {
                                tracing::warn!("Accept error: {}", e);
                            }
                        }
                    }
                });
            }
        }

        Ok(())
    }

    /// TCP 核心分发循环
    pub async fn start_tcp(&self, extractor: Extractor, loop_token: CancellationToken) -> anyhow::Result<()> {
        let listener = TcpListener::bind(self.addr).await?;
        tracing::info!("TCP listener started on {}", self.addr);

        let manager = self.globals.manager.clone();
        let global = self.globals.clone();

        loop {
            tokio::select! {
                _ = loop_token.cancelled() => { break; }
                accept_res = listener.accept() => {
                    let (socket, peer_addr) = match accept_res {
                        Ok(res) => res,
                        Err(e) => { tracing::warn!("Accept error: {}", e); continue; }
                    };

                    let is_h2 = {
                        use tokio::io::AsyncReadExt;
                        let mut buf = [0u8; 24];
                        match socket.peek(&mut buf).await {
                            Ok(n) if n >= 24 => buf.starts_with(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n"),
                            _ => false,
                        }
                    };

                    if is_h2 {
                        let h2_codec_opt = global.h2_codec.read().unwrap().clone();
                        if let Some(h2_codec) = h2_codec_opt {
                            let token = manager.cancel_token.child_token();
                            tokio::spawn(async move {
                                if let Err(e) = h2_codec.handle(socket, peer_addr, token).await {
                                    tracing::warn!("HTTP/2 connection error: {}", e);
                                }
                            });
                            continue;
                        }
                    }

                    let pipeline = ConnectionEntry::default_pipeline::<RawCodec, RawCodec>(
                        peer_addr, true, extractor.clone()
                    );
                    let (conn_token, abort_handle, ctx) = ConnectionEntry::start::<RawCodec, RawCodec, _, _>(
                        manager.cancel_token.clone(), socket, peer_addr, global.clone(), pipeline,
                    );

                    manager.add(peer_addr, abort_handle, conn_token, true, Some(ctx));
                }
            }
        }
        Ok(())
    }

    /// UDP 核心分发循环
    pub async fn start_udp(&self, extractor: Extractor, loop_token: CancellationToken) -> anyhow::Result<()> {
        let socket = Arc::new(UdpSocket::bind(self.addr).await?);
        tracing::info!("UDP listener started on {}", self.addr);

        let rt = self.globals.routers.get_value::<Arc<UdpRouter>>()
            .ok_or_else(|| anyhow::anyhow!("UDP router not found"))?;

        rt.handle::<RawCodec, RawCodec>(self.globals.clone(), socket, extractor).await
    }
}

pub type HTTPServer = Server;