use bytes::{BytesMut, Buf};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, BufReader, BufWriter};
use tokio::net::{
    TcpListener,
    tcp::{OwnedReadHalf, OwnedWriteHalf},
};

use crate::http::req::Request;
use crate::http::res::Response;
use crate::http::router::{Router as HttpRouter, handle_request};
use crate::http::types::{HTTPContext, TypeMap};
use crate::tcp::router::Router as TcpRouter;
use crate::tcp::types::{Codec, Command, Frame, RawCodec}; // 确保引入了 Command
use tokio::sync::Mutex;

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

    pub async fn start(self) -> anyhow::Result<()> {
        let addr = self.addr;
        let server = Arc::new(self);
        let listener = TcpListener::bind(addr).await?;

        println!("[AEX] Multi-protocol server listening on {}", addr);

        loop {
            let (socket, peer_addr) = listener.accept().await?;
            let server_ctx = server.clone();

            tokio::spawn(async move {
                let (mut reader, writer) = socket.into_split();

                if let Some(hr) = &server_ctx.http_router {
                    if Request::is_http_connection(&mut reader)
                        .await
                        .unwrap_or_default()
                    {
                        let reader = BufReader::new(reader);
                        let writer = BufWriter::new(writer);
                        return Self::handle_http(hr.clone(), reader, writer, peer_addr).await;
                    }
                }

                if let Some(tr) = &server_ctx.tcp_router {
                    return Self::handle_tcp(tr.clone(), reader, writer).await;
                }

                Ok::<(), anyhow::Error>(())
            });
        }
    }

    async fn handle_http(
        router: Arc<HttpRouter>,
        reader: BufReader<OwnedReadHalf>,
        writer: BufWriter<OwnedWriteHalf>,
        peer_addr: SocketAddr,
    ) -> anyhow::Result<()> {
        let req = Request::new(reader, peer_addr, "").await?;
        let res = Response::new(writer);
        let mut ctx = HTTPContext {
            req,
            res,
            global: Arc::new(Mutex::new(TypeMap::new())),
            local: TypeMap::new(),
        };

        if handle_request(&router, &mut ctx).await {
            let _ = ctx.res.send().await;
        }
        Ok(())
    }

    async fn handle_tcp(
        router: Arc<TcpRouter<F, C, K>>,
        reader: OwnedReadHalf,
        writer: OwnedWriteHalf,
    ) -> anyhow::Result<()> {
        let mut buf = BytesMut::with_capacity(4096);
        let mut r_opt = Some(reader);
        let mut w_opt = Some(writer);

        loop {
            let n = match r_opt.as_mut() {
                Some(r) => r.read_buf(&mut buf).await?,
                None => break,
            };

            if n == 0 { break; }

            // --- 核心修复：物理分包逻辑 ---
            // 因为你的 Codec::decode 签名是 &[u8]，不处理缓冲区消耗
            // 这里假设通用协议头为 4 字节长度 (BigEndian)
            while buf.len() >= 4 {
                let len = u32::from_be_bytes(buf[..4].try_into().unwrap()) as usize;
                
                if buf.len() < 4 + len {
                    break; // 数据包不全，跳出等待下次读取
                }

                // 1. 消耗长度头
                buf.advance(4);
                // 2. 提取载荷切片
                let data = buf.split_to(len);
                
                // 3. 调用固定的 Codec::decode (由于 F 实现自 Codec)
                if let Ok(frame) = <F as Codec>::decode(&data) {
                    let should_continue = router.handle_frame(frame, &mut r_opt, &mut w_opt).await?;

                    if !should_continue || r_opt.is_none() {
                        return Ok(());
                    }
                }
            }
        }
        Ok(())
    }
}


pub type HTTPServer = AexServer<RawCodec, RawCodec, u32>;