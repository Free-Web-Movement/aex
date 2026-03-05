use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncBufRead, AsyncWrite, BufReader, BufWriter};
use tokio::net::TcpListener;
use tokio::net::UdpSocket;

use crate::communicators::event::{Event, EventCallback};
use crate::communicators::pipe::PipeCallback;
use crate::communicators::spreader::SpreadCallback;
use crate::connection::context::TypeMapExt;
use crate::connection::global::GlobalContext;
use crate::connection::types::IDExtractor;
use crate::http::protocol::method::HttpMethod;
use crate::http::router::Router as HttpRouter;
use crate::tcp::router::Router as TcpRouter;
use crate::tcp::types::{Command, Frame};
use crate::udp::router::Router as UdpRouter;

/// AexServer: 核心多协议服务器
#[derive(Clone)]
pub struct AexServer {
    pub addr: SocketAddr,
    pub globals: Arc<GlobalContext>,
}

impl AexServer {
    pub fn new(addr: SocketAddr) -> Self {
        Self {
            addr,
            // http_router: None,
            // tcp_router: None,
            // udp_router: None,
            globals: Arc::new(GlobalContext::new(addr)),
        }
    }

    pub fn http(&self, router: HttpRouter) -> &Self {
        self.globals.routers.set_value(Arc::new(router));
        // self.http_router = Some(Arc::new(router));
        self
    }

    pub fn tcp(&self, router: TcpRouter) -> &Self {
        self.globals.routers.set_value(Arc::new(router));
        // self.tcp_router = Some(Arc::new(router));
        self
    }

    pub fn udp(&self, router: UdpRouter) -> &Self {
        self.globals.routers.set_value(Arc::new(router));
        // self.udp_router = Some(Arc::new(router));
        self
    }

    /// 🚀 统一启动入口
    pub async fn start<F, C>(&self, extractor: IDExtractor<C>) -> anyhow::Result<()>
    where
        F: Frame + Send + Sync + Clone + 'static,
        C: Command + Send + Sync + 'static,
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
        F: Frame + Send + Sync + Clone + 'static,
        C: Command + Send + Sync + 'static,
    {
        let listener = TcpListener::bind(self.addr).await?;
        println!("[AEX] TCP listener started on {}", self.addr);

        loop {
            let (socket, peer_addr) = listener.accept().await?;
            let server_ctx = Arc::new(self.clone_internal()); // 辅助方法或直接克隆
            let extractor_ctx = extractor.clone();
            let addr = peer_addr.clone();
            let manager = server_ctx.globals.manager.clone();
            let join_handler = tokio::spawn(async move {
                let (mut reader, writer) = socket.into_split();

                // 协议嗅探：HTTP
                let router: Option<Arc<HttpRouter>> = server_ctx.globals.routers.get_value();

                if let Some(hr) = &router
                    && HttpMethod::is_http_connection(&mut reader)
                        .await
                        .unwrap_or_default()
                {
                    let reader = BufReader::new(reader);
                    let writer = BufWriter::new(writer);
                    let rh = hr.clone();
                    return rh
                        .handle(server_ctx.globals.clone(), reader, writer, peer_addr)
                        .await;
                }

                // 自定义 TCP
                let router: Option<Arc<TcpRouter>> = server_ctx.globals.routers.get_value();

                if let Some(tr) = &router {
                    // ⚡ 包装 Buffer 以提升 I/O 性能
                    let buf_reader = BufReader::new(reader);
                    let buf_writer = BufWriter::new(writer);

                    let mut r_opt: Option<Box<dyn AsyncBufRead + Unpin + Send>> =
                        Some(Box::new(buf_reader));
                    let mut w_opt: Option<Box<dyn AsyncWrite + Unpin + Send>> =
                        Some(Box::new(buf_writer));
                    return tr
                        .clone()
                        .handle::<F, C>(
                            addr,
                            server_ctx.globals.clone(),
                            &mut r_opt,
                            &mut w_opt,
                            extractor_ctx,
                        )
                        .await;
                }
                Ok::<(), anyhow::Error>(())
            });
            manager.add(addr, join_handler.abort_handle(), true);
        }
    }

    /// 🛠️ UDP 核心分发循环
    pub async fn start_udp<F, C>(&self, extractor: IDExtractor<C>) -> anyhow::Result<()>
    where
        F: Frame + Send + Sync + Clone + 'static,
        C: Command + Send + Sync + 'static,
    {
        let router: Option<Arc<UdpRouter>> = self.globals.routers.get_value();

        if let Some(router) = &router {
            let socket = Arc::new(UdpSocket::bind(self.addr).await?);
            println!("[AEX] UDP listener started on {}", self.addr);
            let rt = router.clone();
            return rt
                .handle::<F, C>(self.globals.clone(), socket, extractor)
                .await;
        }
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

    /// 注册一个全局管道 (N:1)
    pub async fn pipe<T>(&self, name: &str, callback: PipeCallback<T>) -> &Self
    where
        T: Send + 'static,
    {
        self.globals
            .pipe
            .register(name, callback)
            .await
            .unwrap_or_else(|e| {
                eprintln!("警告: 管道 {} 注册失败: {}", name, e);
            });
        self
    }

    /// 订阅一个全局广播 (1:N)
    pub async fn spread<T>(&self, name: &str, callback: SpreadCallback<T>) -> &Self
    where
        T: Clone + Send + Sync + 'static,
    {
        self.globals
            .spread
            .subscribe(name, callback)
            .await
            .unwrap_or_else(|e| {
                eprintln!("警告: 广播 {} 订阅失败: {}", name, e);
            });
        self
    }

    /// 监听一个全局事件 (M:N)
    pub async fn event<T>(&self, event_name: &str, callback: EventCallback<T>) -> &Self
    where
        T: Clone + Send + Sync + 'static,
    {
        // 调用我们之前实现的异步版 on
        Event::<T>::_on(&self.globals.event, event_name.to_string(), callback).await;
        self
    }
}

pub type HTTPServer = AexServer;
