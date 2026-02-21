use std::net::SocketAddr;
use std::{collections::HashMap, sync::Arc};

use tokio::net::UdpSocket;

use crate::tcp::types::{Command, Frame};
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
        self.handlers.insert(key, Box::new(move |cmd, addr, socket| {
            Box::pin(f(cmd, addr, socket))
        }));
    }
}