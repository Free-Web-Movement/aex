//! # Server
//!
//! Unified multi-protocol server supporting HTTP, TCP, and UDP.
//!
//! ## Example
//!
//! ```rust,ignore
//! use aex::http::router::{NodeType, Router as HttpRouter};
//! use aex::server::HTTPServer;
//! use aex::tcp::types::{Command, RawCodec};
//! use aex::{body, exe, get, route};
//! use std::net::SocketAddr;
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let addr: SocketAddr = "0.0.0.0:8080".parse()?;
//!     let mut router = HttpRouter::new(NodeType::Static("root".into()));
//!
//!     route!(router, get!("/", exe!(|ctx| {
//!         body!(ctx, "Hello!");
//!         true
//!     })));
//!
//!     HTTPServer::new(addr, None)
//!         .http(router)
//!         .start::<RawCodec, RawCodec>(Arc::new(|c| c.id()))
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
use crate::tcp::types::{TCPCommand, TCPFrame};
use crate::udp::router::Router as UdpRouter;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// Multi-protocol server supporting HTTP, TCP, and UDP.
///
/// # Example
///
/// ```rust,ignore
/// Server::new(addr, None)
///     .http(http_router)
///     .tcp(tcp_router)
///     .udp(udp_router)
///     .start::<Frame, Command>(extractor)
///     .await?;
/// ```
#[derive(Clone)]
pub struct Server {
    pub addr: SocketAddr,
    pub globals: Arc<GlobalContext>,
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
        }
    }

    /// Sets the HTTP router.
    pub fn http(&self, router: HttpRouter) -> &Self {
        self.globals.routers.set_value(Arc::new(router));
        self
    }

    /// Sets the TCP router.
    pub fn tcp(&self, router: TcpRouter) -> &Self {
        self.globals.routers.set_value(Arc::new(router));
        self
    }

    /// Sets the UDP router.
    pub fn udp(&self, router: UdpRouter) -> &Self {
        self.globals.routers.set_value(Arc::new(router));
        self
    }

    /// Starts the server with all configured protocols.
    pub async fn start<F, C>(&self, extractor: IDExtractor<C>) -> anyhow::Result<()>
    where
        F: TCPFrame,
        C: TCPCommand,
    {
        let server = Arc::new(self.clone());

        // --- UDP ---
        let udp_token = CancellationToken::new();
        let udp_loop_token = udp_token.clone();
        let server_udp = server.clone();
        let extractor_udp = extractor.clone();

        let udp_handle = tokio::spawn(async move {
            // 注意：内部 start_udp 不要再 add_exit 了！只管监听 token
            let _ = server_udp
                .start_udp::<F, C>(extractor_udp, udp_loop_token)
                .await;
        });
        // 🔑 存入真正的 udp_handle
        server
            .globals
            .add_exit("udp", udp_token, udp_handle.abort_handle())
            .await;

        // --- TCP ---
        let tcp_token = CancellationToken::new();
        let tcp_loop_token = tcp_token.clone();
        let server_tcp = server.clone();

        let tcp_handle = tokio::spawn(async move {
            // 内部 start_tcp 不要再 add_exit 了！
            let _ = server_tcp
                .start_tcp::<F, C>(extractor, tcp_loop_token)
                .await;
        });
        // 🔑 存入真正的 tcp_handle
        server
            .globals
            .add_exit("tcp", tcp_token, tcp_handle.abort_handle())
            .await;

        // 必须 Await，确保 shutdown_all 触发后，start 函数能正常返回
        tcp_handle.await.ok();
        Ok(())
    }

    /// 🛠️ TCP 核心分发循环
    pub async fn start_tcp<F, C>(
        &self,
        extractor: IDExtractor<C>,
        loop_token: CancellationToken,
    ) -> anyhow::Result<()>
    where
        F: TCPFrame,
        C: TCPCommand,
    {
        let listener = TcpListener::bind(self.addr).await?;
        println!("[AEX] TCP listener started on {}", self.addr);

        let manager = self.globals.manager.clone();
        let global = self.globals.clone();
        // let server_addr = self.addr;

        loop {
            tokio::select! {
                    // A. 响应总闸信号
                    _ = loop_token.cancelled() => {
                        println!("[AEX] TCP server main loop received stop signal.");
                        break;
                    }

                    // B. 接收连接
                    accept_res = listener.accept() => {
                        let (socket, peer_addr) = match accept_res {
                            Ok(res) => res,
                            Err(e) => {
                                eprintln!("[AEX] Accept error: {}", e);
                                continue;
                            }
                        };

                        let pipeline = ConnectionEntry::default_pipeline::<F, C>(
                            peer_addr,
                            true,
                            extractor.clone()
                        );
                        // --- D. 调用解耦后的 Pipeline 启动器 ---
                        // 使用 manager 的 token 作为父 token，保证生命周期受控
                        let (conn_token, abort_handle, ctx) = ConnectionEntry::start::<F, C, _, _>(
                            manager.cancel_token.clone(),
                            socket,
                            peer_addr,
                            global.clone(),
                            pipeline,
                        );

                        // --- E. 登记到 ConnectionManager ---
                        // 这里可以直接把 start 返回的两个控制句柄存入 Entry 层
                        manager.add(peer_addr, abort_handle, conn_token, true, Some(ctx));
                    }
                }
        }

        println!("[AEX] TCP server has exited clean.");
        Ok(())
    }

    /// 🛠️ UDP 核心分发循环
    pub async fn start_udp<F, C>(
        &self,
        extractor: IDExtractor<C>,
        task_token: CancellationToken,
    ) -> anyhow::Result<()>
    where
        F: TCPFrame,
        C: TCPCommand,
    {
        // 1. 获取 UDP 路由
        let router: Option<Arc<UdpRouter>> = self.globals.routers.get_value();

        if let Some(router) = &router {
            let socket = Arc::new(UdpSocket::bind(self.addr).await?);
            println!("[AEX] UDP listener started on {}", self.addr);

            let rt = router.clone();
            let globals = self.globals.clone();

            // 5. 使用 select! 包装整个 UDP 处理器
            // 只要 udp_token 被取消，整个 rt.handle 协程会被立即中止
            tokio::select! {
                _ = task_token.cancelled() => {
                    println!("[AEX] UDP server received stop signal.");
                }
                res = rt.handle::<F, C>(globals, socket, extractor) => {
                    if let Err(e) = res {
                        eprintln!("[AEX] UDP Router Execution Error: {}", e);
                    }
                }
            }
        } else {
            println!("[AEX] UDP start failed: No router configured.");
        }

        println!("[AEX] UDP server has exited clean.");
        Ok(())
    }

    // /// 内部辅助：由于 start 需要 Arc<Self>，
    // /// 这里提供一个简单的克隆逻辑用于协程内引用
    // fn clone_internal(&self) -> Self {
    //     Self {
    //         addr: self.addr,
    //         globals: self.globals.clone(),
    //     }
    // }
}

pub type HTTPServer = Server;
