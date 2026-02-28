use std::pin::Pin;

use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

use anyhow::Result;
use bincode;
use serde::{Deserialize, Serialize};

/// C: 业务指令/数据对象 (Command/Message)
use bincode::{Decode, Encode, config, decode_from_slice, encode_to_vec};

#[inline]
pub fn frame_config() -> impl bincode::config::Config {
    config::standard()
        .with_fixed_int_encoding()
        .with_big_endian()
        .with_limit::<1024>() // 设置最大消息大小为 1024
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

pub trait Frame: Codec {
    // 生成校验数据
    fn payload(&self) -> Option<Vec<u8>>;

    // 用于逻辑校验
    fn validate(&self) -> bool {
        true
    }

    // 处理逻辑
    fn command(&self) -> Option<&Vec<u8>>;

    // 用于数据校验
    fn sign<F>(&self, signer: F) -> Vec<u8>
    where
        F: FnOnce(&[u8]) -> Vec<u8>,
    {
        let raw_bytes = Codec::encode(self); // 假设 Codec 提供 encode()
        signer(&raw_bytes)
    }
    fn verify<V>(&self, signature: &[u8], verifier: V) -> bool
    where
        V: FnOnce(&[u8]) -> bool,
    {
        verifier(signature)
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

    // 命令需要发送的二进制数据，可能是加密过的
    fn data(&self) -> &Vec<u8>;
    // 可选实现：是否基于P2P的零信息加密,
    // 默认为false，即不采用加密
    // 如果需要加密，那么必须要进行公钥握手机制。
    // 成功后才能使用
    fn is_trusted(&self) -> bool {
        false
    }
}

/// 🛠️ 纯二进制包装：既不带 ID 也不带冗余结构
#[derive(Serialize, Deserialize, Encode, Decode, Debug, Clone)]
pub struct RawCodec(pub Vec<u8>);
impl Codec for RawCodec {}
impl Frame for RawCodec {
    fn payload(&self) -> Option<Vec<u8>> {
        Some(self.0.clone())
    }
    fn command(&self) -> Option<&Vec<u8>> {
        Some(&self.0)
    }
}

impl Command for RawCodec {
    fn id(&self) -> u32 {
        0 // 纯数据指令，ID 固定为 0
    }
    fn data(&self) -> &Vec<u8> {
        &self.0
    }
}
