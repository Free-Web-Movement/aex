use std::net::SocketAddr;
use std::{collections::HashMap, sync::Arc};

use tokio::net::UdpSocket;

use crate::tcp::types::{Codec, Command, Frame};
use crate::udp::types::PacketExecutor;

pub struct Router<F, C, K = u32>
where
    F: Frame + Send + Sync + 'static,
    C: Command + Send + Sync + 'static,
    K: Eq + std::hash::Hash + Send + Sync + 'static,
{
    pub handlers: HashMap<K, PacketExecutor<C>>, // 使用你之前定义的 PacketExecutor
    pub extractor: Arc<dyn Fn(&C) -> K + Send + Sync>,
    _phantom: std::marker::PhantomData<(F, C)>,
}

impl<F, C, K> Router<F, C, K>
where
    F: Frame + Send + Sync + 'static,
    C: Command + Send + Sync + 'static,
    K: Eq + std::hash::Hash + Send + Sync + 'static,
{
    pub fn new(extractor: impl Fn(&C) -> K + Send + Sync + 'static) -> Self {
        Self {
            handlers: HashMap::new(),
            extractor: Arc::new(extractor),
            _phantom: std::marker::PhantomData,
        }
    }
    // 注册 Handler 的方法与 TCP 类似，只需适配 PacketExecutor 的闭包即可

    pub fn on<FFut, Fut>(&mut self, key: K, f: FFut)
    where
        FFut: Fn(C, SocketAddr, Arc<UdpSocket>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = anyhow::Result<bool>> + Send + 'static,
    {
        self.handlers.insert(
            key,
            Box::new(move |cmd, addr, socket| Box::pin(f(cmd, addr, socket))),
        );
    }

    pub async fn handle(self: Arc<Self>, socket: Arc<UdpSocket>) -> anyhow::Result<()> {
        let mut buf = [0u8; 65535]; // UDP 最大报文长度
        loop {
            let (n, peer_addr) = socket.recv_from(&mut buf).await?;
            let data = buf[..n].to_vec();

            let router_ctx = self.clone();
            let socket_ctx = socket.clone();

            // UDP 通常为无状态，直接 spawn 处理每个包
            tokio::spawn(async move {
                // 1. 解码为 Frame (Codec::decode)
                if let Ok(frame) = <F as Codec>::decode(&data) {
                    if !frame.validate() {
                        return;
                    }

                    // 2. 获取 Payload 并解码为 Command
                    if let Some(payload) = frame.command()
                        && let Ok(cmd) = <C as Codec>::decode(&payload)
                    {
                        let key = (router_ctx.extractor)(&cmd);

                        // 3. 路由并执行逻辑
                        if let Some(handler) = router_ctx.handlers.get(&key) {
                            // 执行 PacketExecutor (Vec<u8>, SocketAddr, Arc<UdpSocket>)
                            let _ = handler(cmd, peer_addr, socket_ctx).await;
                        }
                    }
                }
            });
        }
    }
}
