use std::{net::SocketAddr, pin::Pin, sync::Arc};

use tokio::net::UdpSocket;

// ⚡ 修改点：增加泛型参数 <C>
pub type PacketExecutor<C> = Box<
    dyn Fn(
            C,              // 之前这里可能是 Vec<u8>，必须改为 C
            SocketAddr,
            Arc<UdpSocket>,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<bool>> + Send>>
        + Send
        + Sync,
>;