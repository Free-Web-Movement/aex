use std::{
    net::SocketAddr,
    pin::Pin,
    sync::Arc,
};

use tokio::net::{
    UdpSocket,
    tcp::{OwnedReadHalf, OwnedWriteHalf},
};

use anyhow::Result;
use bincode;
use serde::{Deserialize, Serialize};


/// C: 业务指令/数据对象 (Command/Message)

use bincode::{config, decode_from_slice, encode_to_vec, Decode, Encode};

#[inline]
pub fn frame_config() -> impl bincode::config::Config {
    config::standard().with_fixed_int_encoding().with_big_endian()
}

/// ⚡ 修正后的 Codec trait
/// 注意：为了配合 bincode 2.0，我们需要同时满足 serde 和 bincode 的宏要求
pub trait Codec: Serialize + for<'de> Deserialize<'de> + Encode + Decode<()> + Sized {
    /// 序列化
    fn encode(&self) -> Vec<u8> {
        // 使用 bincode 2.0 标准配置进行编码
        encode_to_vec(self, frame_config()).expect("serialize failed")
    }

    /// 反序列化
    fn decode(data: &[u8]) -> Result<Self> {
        // bincode 2.0 返回 (Object, read_length)
        let (decoded, _): (Self, usize) = decode_from_slice(data, frame_config())
            .map_err(|e| anyhow::anyhow!("decode failed: {}", e))?;
        Ok(decoded)
    }
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
    // 核心属性：获取该帧内部包裹的原始字节负载
    // 用于交给 Command::decode 进行进一步解析
    // 返回 Option，如果没有子指令，返回 None；如果有，返回 Some(&[u8])
    fn payload(&self) -> Option<&[u8]>;
    /// 可选：获取帧头信息或校验状态，默认返回 true
    fn validate(&self) -> bool {
        true
    }
    // 按照你之前的要求，返回 Option<Vec<u8>>
    fn handle(&self) -> Option<Vec<u8>>;
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

/// 🛠️ 纯二进制包装：既不带 ID 也不带冗余结构
#[derive(Serialize, Deserialize, Encode, Decode, Debug, Clone)]
pub struct RawCodec(pub Vec<u8>);
impl Codec for RawCodec {}
impl Frame for RawCodec {
    fn payload(&self) -> Option<&[u8]> {
        Some(&self.0)
    }
    fn handle(&self) -> Option<Vec<u8>> {
        Some(self.0.clone())
    }
}

impl Command for RawCodec {
    fn id(&self) -> u32 {
        0 // 纯数据指令，ID 固定为 0
    }
}