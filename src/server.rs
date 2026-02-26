use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, BufReader, BufWriter};
use tokio::net::UdpSocket;
use tokio::net::{
    TcpListener,
    tcp::{OwnedReadHalf, OwnedWriteHalf},
};

use crate::connection::context::{GlobalContext, HTTPContext};
use crate::http::protocol::method::HttpMethod;
use crate::http::router::{Router as HttpRouter, handle_request};
use crate::tcp::router::Router as TcpRouter;
use crate::tcp::types::{Codec, Command, Frame, RawCodec}; // 确保引入了 Command
use crate::udp::router::Router as UdpRouter;
use tokio::sync::Mutex;

pub const SERVER_NAME: &str = "Aex/1.0";

/// AexServer: 核心多协议服务器
pub struct AexServer<F, C, K = u32>
where
    F: Frame + Send + Sync + 'static,
    C: Command + Send + Sync + 'static, // 统一使用 Command 约束
    K: Eq + std::hash::Hash + Send + Sync + 'static,
{
    pub addr: SocketAddr,
    pub http_router: Option<Arc<HttpRouter>>,
    pub tcp_router: Option<Arc<TcpRouter<F, C, K>>>,
    pub udp_router: Option<Arc<UdpRouter<F, C, K>>>,
    pub globals: Arc<Mutex<GlobalContext>>,
    _phantom: std::marker::PhantomData<(F, C)>, // 修正 PhantomData 包含 C
}

impl<F, C, K> AexServer<F, C, K>
where
    F: Frame + Send + Sync + 'static,
    C: Command + Send + Sync + 'static,
    K: Eq + std::hash::Hash + Send + Sync + 'static,
{
    pub fn new(addr: SocketAddr) -> Self {
        Self {
            addr,
            http_router: None,
            tcp_router: None,
            udp_router: None,
            globals: Arc::new(Mutex::new(GlobalContext::new(addr))),
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn http(mut self, router: HttpRouter) -> Self {
        self.http_router = Some(Arc::new(router));
        self
    }

    pub fn tcp(mut self, router: TcpRouter<F, C, K>) -> Self {
        self.tcp_router = Some(Arc::new(router));
        self
    }

    pub fn udp(mut self, router: UdpRouter<F, C, K>) -> Self {
        self.udp_router = Some(Arc::new(router));
        self
    }

    /// 🚀 统一启动入口
    pub async fn start(self) -> anyhow::Result<()> {
        let server = Arc::new(self);

        // 1. 启动 UDP 监听 (后台协程)
        if server.udp_router.is_some() {
            let server_udp = server.clone();
            tokio::spawn(async move {
                if let Err(e) = server_udp.start_udp().await {
                    eprintln!("[AEX] UDP Server Error: {}", e);
                }
            });
        }

        // 2. 启动 TCP 监听 (主协程阻塞)
        server.start_tcp().await
    }

    /// 🛠️ TCP 核心分发循环
    pub async fn start_tcp(&self) -> anyhow::Result<()> {
        let listener = TcpListener::bind(self.addr).await?;
        println!("[AEX] TCP listener started on {}", self.addr);

        loop {
            let (socket, peer_addr) = listener.accept().await?;
            let server_ctx = Arc::new(self.clone_internal()); // 辅助方法或直接克隆

            tokio::spawn(async move {
                let (mut reader, writer) = socket.into_split();

                // 协议嗅探：HTTP
                if let Some(hr) = &server_ctx.http_router {
                    if HttpMethod::is_http_connection(&mut reader)
                        .await
                        .unwrap_or_default()
                    {
                        let reader = BufReader::new(reader);
                        let writer = BufWriter::new(writer);
                        return Self::handle_http(hr.clone(), reader, writer, peer_addr).await;
                    }
                }

                // 自定义 TCP
                if let Some(tr) = &server_ctx.tcp_router {
                    return Self::handle_tcp(tr.clone(), reader, writer).await;
                }

                Ok::<(), anyhow::Error>(())
            });
        }
    }

    /// 🛠️ UDP 核心分发循环
    pub async fn start_udp(&self) -> anyhow::Result<()> {
        if let Some(router) = &self.udp_router {
            let socket = Arc::new(UdpSocket::bind(self.addr).await?);
            println!("[AEX] UDP listener started on {}", self.addr);

            return Self::handle_udp(router.clone(), socket).await;
        }
        Ok(())
    }

    /// 内部辅助：由于 start 需要 Arc<Self>，
    /// 这里提供一个简单的克隆逻辑用于协程内引用
    fn clone_internal(&self) -> Self {
        Self {
            addr: self.addr,
            http_router: self.http_router.clone(),
            tcp_router: self.tcp_router.clone(),
            udp_router: self.udp_router.clone(),
            globals: self.globals.clone(),
            _phantom: std::marker::PhantomData,
        }
    }

    async fn handle_http(
        router: Arc<HttpRouter>,
        reader: BufReader<OwnedReadHalf>,
        writer: BufWriter<OwnedWriteHalf>,
        peer_addr: SocketAddr,
    ) -> anyhow::Result<()> {
        // let req = Request::new(reader, peer_addr, "").await?;

        // let res = Response::new(writer);
        let mut ctx = HTTPContext::new(
            reader,
            writer,
            Arc::new(GlobalContext::new(peer_addr)),
            peer_addr,
        );
        ctx.req().await.parse_to_local().await?;
        
        // handle_request 返回 true 表示所有中间件和 Handler 正常通过
        // 返回 false 表示被拦截（如 validator 发现类型不匹配）
        if handle_request(&router, &mut ctx).await {
            // 🟢 正常出口
            ctx.res().send_response().await?;
        } else {
            // 🔴 错误/拦截出口
            // 此时 send_failure 会读取 validator 写入的 "'{}' is not a valid boolean"
            ctx.res().send_failure().await?;
        }
        Ok(())
    }

    async fn handle_tcp(
        router: Arc<TcpRouter<F, C, K>>,
        reader: OwnedReadHalf,
        writer: OwnedWriteHalf,
    ) -> anyhow::Result<()> {
        let mut r_opt = Some(reader);
        let mut w_opt = Some(writer);

        // 固定的轻量级缓冲区，仅用于读取 Frame 头
        let mut buf = vec![0u8; 1024];

        loop {
            // 尝试获取 reader，如果被 handler 接管走了，这里就退出循环
            let r = match r_opt.as_mut() {
                Some(r) => r,
                None => {
                    break;
                }
            };

            // 1. 读取一次数据，期望是一个完整的 Frame
            let n = r.read(&mut buf).await?;
            if n == 0 {
                break;
            }

            let data = &buf[..n];

            // 2. 解码 Frame
            let frame_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                <F as Codec>::decode(data)
            }));

            match frame_result {
                Ok(Ok(frame)) => {
                    // 3. 分发给 Router
                    // 如果 Handler 需要读后续数据，它会通过 r_opt.take() 拿走 Reader 的所有权
                    let should_continue =
                        router.handle_frame(frame, &mut r_opt, &mut w_opt).await?;

                    // 4. 检查 Reader 是否还在，或者 Handler 是否要求关闭
                    if !should_continue || r_opt.is_none() {
                        break;
                    }
                }
                Ok(Err(e)) => {
                    eprintln!("[AEX] 解码业务逻辑失败: {}", e);
                }
                Err(_) => {
                    eprintln!("[AEX] 严重错误：解码器发生了崩溃 (Panic)！已丢弃该包并隔离。");
                }
            }
        }
        Ok(())
    }

    pub async fn handle_udp(
        router: Arc<UdpRouter<F, C, K>>,
        socket: Arc<UdpSocket>,
    ) -> anyhow::Result<()> {
        let mut buf = [0u8; 65535]; // UDP 最大报文长度
        loop {
            let (n, peer_addr) = socket.recv_from(&mut buf).await?;
            let data = buf[..n].to_vec();

            let router_ctx = router.clone();
            let socket_ctx = socket.clone();

            // UDP 通常为无状态，直接 spawn 处理每个包
            tokio::spawn(async move {
                // 1. 解码为 Frame (Codec::decode)
                if let Ok(frame) = <F as Codec>::decode(&data) {
                    if !frame.validate() {
                        return;
                    }

                    // 2. 获取 Payload 并解码为 Command
                    if let Some(payload) = frame.handle() {
                        if let Ok(cmd) = <C as Codec>::decode(&payload) {
                            let key = (router_ctx.extractor)(&cmd);

                            // 3. 路由并执行逻辑
                            if let Some(handler) = router_ctx.handlers.get(&key) {
                                // 执行 PacketExecutor (Vec<u8>, SocketAddr, Arc<UdpSocket>)
                                let _ = handler(cmd, peer_addr, socket_ctx).await;
                            }
                        }
                    }
                }
            });
        }
    }
}

pub type HTTPServer = AexServer<RawCodec, RawCodec, u32>;
