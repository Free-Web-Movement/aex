use std::sync::Arc;

use crate::{
    connection::context::Context,
    http::middlewares::websocket::WebSocket,
    tcp::types::{Codec, Command, Frame},
};
use bincode::{Decode, Encode};
use bytes::{BufMut, BytesMut};
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use tokio_util::codec::{Decoder, Encoder};

// --- WSFrame 适配 ---
#[derive(Debug, Clone, Decode, Encode, Deserialize, Serialize, PartialEq)]
pub enum WSFrame {
    /// 0x0: 分片后续帧
    Continuation(Vec<u8>),
    /// 0x1: 文本帧
    Text(String),
    /// 0x2: 二进制帧
    Binary(Vec<u8>),
    /// 0x3 - 0x7: 预留非控制
    ReservedNonControl(u8, Vec<u8>),
    /// 0x8: 关闭帧 (状态码, 原因)
    Close(u16, Option<String>),
    /// 0x9: Ping
    Ping(Vec<u8>),
    /// 0xA: Pong
    Pong(Vec<u8>),
    /// 0xB - 0xF: 预留控制位
    ReservedControl(u8, Vec<u8>),
}

impl Codec for WSFrame {}

// --- 实现 Frame Trait ---
impl Frame for WSFrame {
    fn payload(&self) -> Option<Vec<u8>> {
        match self {
            WSFrame::Text(s) => Some(s.as_bytes().to_vec()),
            WSFrame::Binary(b)
            | WSFrame::Continuation(b)
            | WSFrame::Ping(b)
            | WSFrame::Pong(b)
            | WSFrame::ReservedNonControl(_, b)
            | WSFrame::ReservedControl(_, b) => Some(b.clone()),
            WSFrame::Close(_, _) => None,
        }
    }

    // 指令映射到自身
    fn command(&self) -> Option<&Vec<u8>> {
        None
    }
}

// --- 实现 Command Trait ---
impl Command for WSFrame {
    fn id(&self) -> u32 {
        match self {
            WSFrame::Continuation(_) => 0x0,
            WSFrame::Text(_) => 0x1,
            WSFrame::Binary(_) => 0x2,
            WSFrame::ReservedNonControl(op, _) => *op as u32,
            WSFrame::Close(_, _) => 0x8,
            WSFrame::Ping(_) => 0x9,
            WSFrame::Pong(_) => 0xa,
            WSFrame::ReservedControl(op, _) => *op as u32,
        }
    }

    fn data(&self) -> &Vec<u8> {
        static EMPTY: Vec<u8> = Vec::new();
        match self {
            // 注意：Text 这里需要转 Vec 的话会涉及引用问题，
            // 如果 AEX 框架允许，建议 Text 内部也存 Vec<u8> 以实现真正的零拷贝 data()
            WSFrame::Binary(b)
            | WSFrame::Continuation(b)
            | WSFrame::Ping(b)
            | WSFrame::Pong(b)
            | WSFrame::ReservedNonControl(_, b)
            | WSFrame::ReservedControl(_, b) => b,
            _ => &EMPTY,
        }
    }
}

pub struct WSCodec;
impl Decoder for WSCodec {
    type Item = WSFrame;
    type Error = anyhow::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.len() < 2 {
            return Ok(None);
        }

        let first = src[0];
        let second = src[1];

        let _fin = (first & 0x80) != 0;
        let opcode = first & 0x0f;
        let masked = (second & 0x80) != 0;
        let mut payload_len = (second & 0x7f) as usize;
        let mut head_len = 2;

        // 1. 解析扩展长度 (已支持 126/127 边界)
        if payload_len == 126 {
            if src.len() < 4 {
                return Ok(None);
            }
            payload_len = u16::from_be_bytes([src[2], src[3]]) as usize;
            head_len += 2;
        } else if payload_len == 127 {
            if src.len() < 10 {
                return Ok(None);
            }
            payload_len = u64::from_be_bytes(src[2..10].try_into()?) as usize;
            head_len += 8;
        }

        // 2. 解析 Mask 偏移
        let mask_offset = head_len;
        if masked {
            head_len += 4;
        }

        // 3. 检查半包
        if src.len() < head_len + payload_len {
            return Ok(None);
        }

        // 4. 提取数据
        let header = src.split_to(head_len);
        let mut payload = src.split_to(payload_len).to_vec();

        // 5. 解掩码
        if masked {
            let mask = &header[mask_offset..mask_offset + 4];
            for i in 0..payload_len {
                payload[i] ^= mask[i % 4];
            }
        }

        // 6. 转换为统一枚举 (全面覆盖 Opcode)
        match opcode {
            0x0 => Ok(Some(WSFrame::Continuation(payload))),
            0x1 => Ok(Some(WSFrame::Text(String::from_utf8(payload)?))),
            0x2 => Ok(Some(WSFrame::Binary(payload))),
            0x8 => {
                let (code, reason) = WebSocket::parse_close_payload(&payload)?;
                Ok(Some(WSFrame::Close(code, reason.map(|s| s.to_string()))))
            }
            0x9 => Ok(Some(WSFrame::Ping(payload))),
            0xa => Ok(Some(WSFrame::Pong(payload))),
            // 显式处理合法的预留位，其余全部视为 Unsupported
            0x3..=0x7 => Ok(Some(WSFrame::ReservedNonControl(opcode, payload))),
            0xb..=0xf => Ok(Some(WSFrame::ReservedControl(opcode, payload))),
            // 只有当上述逻辑被修改（例如删掉了 Reserved 映射）或者 opcode 提取逻辑失效时才会触发
            _ => Err(anyhow::anyhow!("Unsupported opcode: 0x{:x}", opcode)),
        }
    }
}

impl Encoder<WSFrame> for WSCodec {
    type Error = anyhow::Error;

    fn encode(&mut self, item: WSFrame, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let (opcode, payload) = match item {
            WSFrame::Continuation(b) => (0x0u8, b),
            WSFrame::Text(s) => (0x1u8, s.into_bytes()),
            WSFrame::Binary(b) => (0x2u8, b),
            WSFrame::ReservedNonControl(op, b) => (op, b),
            WSFrame::Close(code, reason) => {
                let mut p = code.to_be_bytes().to_vec();
                if let Some(r) = reason {
                    p.extend_from_slice(r.as_bytes());
                }
                (0x8u8, p)
            }
            WSFrame::Ping(b) => (0x9u8, b),
            WSFrame::Pong(b) => (0xau8, b),
            WSFrame::ReservedControl(op, b) => (op, b),
        };

        // 目前默认 FIN = 1。如果后续要做分片发送，可根据 Continuation 逻辑动态调整
        dst.put_u8(0x80 | (opcode & 0x0f));

        let len = payload.len();
        if len < 126 {
            dst.put_u8(len as u8);
        } else if len <= 65535 {
            dst.put_u8(126);
            dst.put_u16(len as u16);
        } else {
            dst.put_u8(127);
            dst.put_u64(len as u64);
        }

        dst.extend_from_slice(&payload);
        Ok(())
    }
}

pub type WebSocketHandler =
    Arc<dyn (Fn(&WebSocket, &mut Context, WSFrame) -> BoxFuture<'static, bool>) + Send + Sync>;

pub type TextHandler =
    Arc<dyn (Fn(&WebSocket, &mut Context, String) -> BoxFuture<'static, bool>) + Send + Sync>;

pub type BinaryHandler =
    Arc<dyn (Fn(&WebSocket, &mut Context, Vec<u8>) -> BoxFuture<'static, bool>) + Send + Sync>;
