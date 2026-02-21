use bytes::BytesMut;
use std::{
    any::{Any, TypeId},
    collections::HashMap,
    net::SocketAddr,
    pin::Pin,
    sync::Arc,
};

use tokio::net::{
    UdpSocket,
    tcp::{OwnedReadHalf, OwnedWriteHalf},
};

/// C: 业务指令/数据对象 (Command/Message)
pub trait Codec: Sized {
    fn decode(src: &mut BytesMut) -> Option<Self>;
    fn encode(self, dst: &mut BytesMut);
}

pub type StreamExecutor = Box<
    dyn Fn(
            OwnedReadHalf,
            OwnedWriteHalf,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<bool>> + Send>>
        + Send
        + Sync,
>;

pub type PacketExecutor = Box<
    dyn Fn(
            Vec<u8>,
            SocketAddr,
            Arc<UdpSocket>,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<bool>> + Send>>
        + Send
        + Sync,
>;

/// Frame 继承自 Codec，它是物理层的容器
pub trait Frame: Codec {
    /// 核心属性：获取该帧内部包裹的原始字节负载
    /// 用于交给 Command::decode 进行进一步解析
    fn payload(&self) -> &[u8];

    /// 可选：获取帧头信息或校验状态
    fn validate(&self) -> bool {
        true
    }
}

pub trait Command: Codec {
    // 属性名推荐使用`_id`，以示与id区别
    // 实现可以以通过`impl Command for MyCommand { fn id() -> u32 { return self._id } }`来指定指令id
    fn id(&self) -> u32;

    // 可选实现：逻辑校验，默认总是合法
    fn validate(&self) -> bool {
        true
    }
}
