use crate::connection::context::{BoxReader, BoxWriter, TypeMapExt};
use crate::connection::global::GlobalContext;
use crate::connection::types::IDExtractor;
use crate::crypto::session_key_manager::PairedSessionKey;
use crate::http::protocol::method::HttpMethod;
use crate::http::router::Router as HttpRouter;
use crate::tcp::router::Router as TcpRouter;
use crate::tcp::types::{TCPCommand, TCPFrame};
use crate::udp::router::Router as UdpRouter;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{BufReader, BufWriter};
use tokio::net::TcpListener;
use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// AexServer: 核心多协议服务器
#[derive(Clone)]
pub struct AexServer {
    pub addr: SocketAddr,
    pub globals: Arc<GlobalContext>,
}

impl AexServer {
    pub fn new(addr: SocketAddr, globals: Option<Arc<GlobalContext>>) -> Self {
        Self {
            addr,
            globals: globals.unwrap_or(Arc::new(GlobalContext::new(
                addr,
                Some(Arc::new(Mutex::new(PairedSessionKey::new(16)))),
            ))),
        }
    }

    pub fn http(&self, router: HttpRouter) -> &Self {
        self.globals.routers.set_value(Arc::new(router));
        self
    }

    pub fn tcp(&self, router: TcpRouter) -> &Self {
        self.globals.routers.set_value(Arc::new(router));
        self
    }

    pub fn udp(&self, router: UdpRouter) -> &Self {
        self.globals.routers.set_value(Arc::new(router));
        self
    }

    /// 🚀 统一启动入口
    pub async fn start<F, C>(&self, extractor: IDExtractor<C>) -> anyhow::Result<()>
    where
        F: TCPFrame,
        C: TCPCommand,
    {
        let server = Arc::new(self.clone());

        let extractor_udp = extractor.clone();

        // 1. 启动 UDP 监听 (后台协程)
        let router: Option<Arc<UdpRouter>> = server.globals.routers.get_value();
        if router.is_some() {
            let server_udp = server.clone();
            tokio::spawn(async move {
                if let Err(e) = server_udp.start_udp::<F, C>(extractor_udp.clone()).await {
                    eprintln!("[AEX] UDP Server Error: {}", e);
                }
            });
        }

        // 2. 启动 TCP 监听 (主协程阻塞)
        server.start_tcp::<F, C>(extractor).await
    }

    /// 🛠️ TCP 核心分发循环
    pub async fn start_tcp<F, C>(&self, extractor: IDExtractor<C>) -> anyhow::Result<()>
    where
        F: TCPFrame,
        C: TCPCommand,
    {
        let listener = TcpListener::bind(self.addr).await?;
        println!("[AEX] TCP listener started on {}", self.addr);

        // 1. 创建服务器级总 Token (Server-Level)
        let server_token = CancellationToken::new();
        let loop_token = server_token.clone();

        // 2. 获取当前任务的 AbortHandle
        // 假设 start_tcp 是在主 spawn 中运行，我们需要将其存入 Global 以便 shutdown_all 调用
        let current_handle = tokio::task::spawn(async {}).abort_handle();

        // 3. 注册到 GlobalContext
        self.globals
            .add_exit("tcp", server_token, current_handle)
            .await;

        loop {
            tokio::select! {
                // A. 响应来自 GlobalContext.shutdown_all() 的总闸信号
                _ = loop_token.cancelled() => {
                    println!("[AEX] TCP server main loop received stop signal.");
                    break;
                }

                // B. 正常接收连接
                accept_res = listener.accept() => {
                    let (socket, peer_addr) = match accept_res {
                        Ok(res) => res,
                        Err(e) => {
                            eprintln!("[AEX] Accept error: {}", e);
                            continue;
                        }
                    };

                    // --- C. 派生连接分闸 (Connection-Level) ---
                    // 关键：conn_token 继承自 loop_token。
                    // 当全局总闸关掉时，所有派生出的 conn_token 会同步触发取消。
                    let conn_token = loop_token.child_token();

                    let server_ctx = Arc::new(self.clone_internal());
                    let extractor_ctx = extractor.clone();
                    let manager = server_ctx.globals.manager.clone();
                    let task_token = conn_token.clone();

                    let join_handler = tokio::spawn(async move {
                        tokio::select! {
                            // 监听当前连接的退出信号
                            _ = task_token.cancelled() => {
                                // println!("[AEX] Connection to {} cancelled", peer_addr);
                                Ok::<(), anyhow::Error>(())
                            }
                            // 执行业务逻辑
                            res = async {
                                let (mut reader, writer) = socket.into_split();

                                // 1. 协议嗅探：HTTP
                                let router: Option<Arc<HttpRouter>> = server_ctx.globals.routers.get_value();
                                if let Some(hr) = &router
                                    && HttpMethod::is_http_connection(&mut reader)
                                        .await
                                        .unwrap_or_default()
                                {
                                    let reader = BufReader::new(reader);
                                    let writer = BufWriter::new(writer);
                                    return hr.clone().handle(server_ctx.globals.clone(), reader, writer, peer_addr).await;
                                }

                                // 2. 自定义 TCP 处理
                                let router: Option<Arc<TcpRouter>> = server_ctx.globals.routers.get_value();
                                if let Some(tr) = &router {
                                    let mut r_opt: Option<BoxReader> = Some(Box::new(BufReader::new(reader)));
                                    let mut w_opt: Option<BoxWriter> = Some(Box::new(BufWriter::new(writer)));
                                    return tr
                                        .clone()
                                        .handle::<F, C>(
                                            peer_addr,
                                            server_ctx.globals.clone(),
                                            &mut r_opt,
                                            &mut w_opt,
                                            extractor_ctx,
                                        )
                                        .await;
                                }
                                Ok(())
                            } => res
                        }
                    });

                    // --- D. 存入 ConnectionManager ---
                    // 这里记录每个 Peer 的独立控制权
                    manager.add(peer_addr, join_handler.abort_handle(), conn_token, true, None, None);
                }
            }
        }

        println!("[AEX] TCP server has exited clean.");
        Ok(())
    }

    // /// 🛠️ UDP 核心分发循环
    // pub async fn start_udp<F, C>(&self, extractor: IDExtractor<C>) -> anyhow::Result<()>
    // where
    //     F: TCPFrame,
    //     C: TCPCommand,
    // {
    //     let router: Option<Arc<UdpRouter>> = self.globals.routers.get_value();

    //     if let Some(router) = &router {
    //         let socket = Arc::new(UdpSocket::bind(self.addr).await?);
    //         println!("[AEX] UDP listener started on {}", self.addr);
    //         let rt = router.clone();
    //         return rt
    //             .handle::<F, C>(self.globals.clone(), socket, extractor)
    //             .await;
    //     }
    //     Ok(())
    // }

    /// 🛠️ UDP 核心分发循环
    pub async fn start_udp<F, C>(&self, extractor: IDExtractor<C>) -> anyhow::Result<()>
    where
        F: TCPFrame,
        C: TCPCommand,
    {
        // 1. 获取 UDP 路由
        let router: Option<Arc<UdpRouter>> = self.globals.routers.get_value();

        if let Some(router) = &router {
            // 2. 准备服务级 Token 与 Handle
            let udp_token = tokio_util::sync::CancellationToken::new();
            let task_token = udp_token.clone();

            // 获取当前任务句柄用于物理关停登记
            let current_handle = tokio::task::spawn(async {}).abort_handle();

            // 3. 注册到 GlobalContext
            self.globals
                .add_exit("udp", udp_token, current_handle)
                .await;

            // 4. 绑定端口
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

    /// 内部辅助：由于 start 需要 Arc<Self>，
    /// 这里提供一个简单的克隆逻辑用于协程内引用
    fn clone_internal(&self) -> Self {
        Self {
            addr: self.addr,
            globals: self.globals.clone(),
        }
    }
}

pub type HTTPServer = AexServer;
