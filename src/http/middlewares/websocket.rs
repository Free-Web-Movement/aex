use crate::{
    connection::context::{HTTPContext, TypeMapExt},
    http::{
        meta::HttpMetadata, protocol::{header::HeaderKey, method::HttpMethod}, types::{BinaryHandler, Executor, TextHandler}
    },
};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use sha1::{Digest, Sha1};
use std::{collections::HashMap, sync::Arc};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, BufReader, BufWriter},
    net::tcp::{OwnedReadHalf, OwnedWriteHalf},
    sync::Mutex,
};

pub struct WebSocket {
    pub on_text: Option<TextHandler>,
    pub on_binary: Option<BinaryHandler>,
}

impl WebSocket {
    /// 判断请求是否是 WebSocket 握手
    pub fn check(method: HttpMethod, headers: &HashMap<HeaderKey, String>) -> bool {
        if method != HttpMethod::GET {
            return false;
        }

        let upgrade = headers
            .get(&HeaderKey::Upgrade)
            .map(|v| v.eq_ignore_ascii_case("websocket"))
            .unwrap_or(false);

        let connection = headers
            .get(&HeaderKey::Connection)
            .map(|v| v.to_ascii_lowercase().contains("upgrade"))
            .unwrap_or(false);

        upgrade && connection
    }

    /// 完成 WebSocket 握手
    pub async fn handshake(
        writer: &mut BufWriter<OwnedWriteHalf>,
        headers: &HashMap<HeaderKey, String>,
    ) -> anyhow::Result<()> {
        let key = headers
            .get(&HeaderKey::SecWebSocketKey)
            .ok_or_else(|| anyhow::anyhow!("missing Sec-WebSocket-Key"))?;

        let mut sha = Sha1::new();
        sha.update(key.as_bytes());
        sha.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
        let accept_key = STANDARD.encode(sha.finalize());

        let response = format!(
            "HTTP/1.1 101 Switching Protocols\r\n\
            Upgrade: websocket\r\n\
            Connection: Upgrade\r\n\
            Sec-WebSocket-Accept: {}\r\n\r\n",
            accept_key
        );

        writer.write_all(response.as_bytes()).await?;
        writer.flush().await?;
        Ok(())
    }

    /// 发送文本消息
    pub async fn send_text(
        writer: &mut BufWriter<OwnedWriteHalf>,
        msg: &str,
    ) -> anyhow::Result<()> {
        Self::send_frame(writer, 0x1, msg.as_bytes()).await
    }

    /// 发送二进制消息
    pub async fn send_binary(
        writer: &mut BufWriter<OwnedWriteHalf>,
        payload: &[u8],
    ) -> anyhow::Result<()> {
        Self::send_frame(writer, 0x2, payload).await
    }

    /// 发送 ping
    pub async fn send_ping(writer: &mut BufWriter<OwnedWriteHalf>) -> anyhow::Result<()> {
        Self::send_frame(writer, 0x9, &[]).await
    }

    /// 发送 pong
    pub async fn send_pong(writer: &mut BufWriter<OwnedWriteHalf>) -> anyhow::Result<()> {
        Self::send_frame(writer, 0xa, &[]).await
    }

    /// 关闭连接
    pub async fn close(
        writer: &mut BufWriter<OwnedWriteHalf>,
        code: u16,
        reason: Option<&str>,
    ) -> anyhow::Result<()> {
        let reason_bytes = reason.unwrap_or("").as_bytes();
        let mut payload = Vec::with_capacity(2 + reason_bytes.len());
        payload.extend_from_slice(&code.to_be_bytes());
        payload.extend_from_slice(reason_bytes);

        Self::send_frame(writer, 0x8, &payload).await?;
        writer.flush().await?;
        Ok(())
    }

    /// 读取一个完整消息，返回 (opcode, payload)
    /// 自动处理 ping/pong
    pub async fn read_full(
        reader: &mut BufReader<OwnedReadHalf>,
        writer: &mut BufWriter<OwnedWriteHalf>,
    ) -> anyhow::Result<(u8, Vec<u8>)> {
        loop {
            let mut header = [0u8; 2];
            reader.read_exact(&mut header).await?;

            let fin = (header[0] & 0x80) != 0;
            let opcode = header[0] & 0x0f;
            let masked = (header[1] & 0x80) != 0;
            let mut len = (header[1] & 0x7f) as usize;

            // ❌ 拒绝 fragmentation
            if !fin || opcode == 0x0 {
                Self::close(writer, 1002, Some("fragmentation not supported")).await?;
                anyhow::bail!("fragmented frame not supported");
            }

            // server 必须接收 masked frame
            if !masked {
                Self::close(writer, 1002, Some("unmasked frame")).await?;
                anyhow::bail!("protocol error");
            }

            if !fin {
                Self::close(writer, 1003, Some("fragmentation not supported")).await?;
                anyhow::bail!("fragmentation");
            }

            // control frame 不能有 payload > 125
            if opcode >= 0x8 && len > 125 {
                Self::close(writer, 1002, Some("invalid control frame")).await?;
                anyhow::bail!("invalid control frame");
            }

            if len == 126 {
                let mut b = [0u8; 2];
                reader.read_exact(&mut b).await?;
                len = u16::from_be_bytes(b) as usize;
            } else if len == 127 {
                let mut b = [0u8; 8];
                reader.read_exact(&mut b).await?;
                len = u64::from_be_bytes(b) as usize;
            }

            let mut mask = [0u8; 4];
            reader.read_exact(&mut mask).await?;

            let mut payload = vec![0u8; len];
            reader.read_exact(&mut payload).await?;

            for i in 0..len {
                payload[i] ^= mask[i % 4];
            }

            match opcode {
                0x9 => {
                    // ping → 自动 pong
                    Self::send_pong(writer).await?;
                    continue;
                }

                0xa => {
                    // pong → 忽略
                    continue;
                }

                0x8 => {
                    let (code, reason) = match Self::parse_close_payload(&payload) {
                        Ok(v) => v,
                        Err(_) => {
                            Self::close(writer, 1002, Some("protocol error")).await?;
                            anyhow::bail!("invalid close frame");
                        }
                    };

                    Self::close(writer, code, reason).await?;
                    anyhow::bail!("connection closed");
                }

                0x1 | 0x2 => {
                    // text / binary
                    return Ok((opcode, payload));
                }

                _ => {
                    Self::close(writer, 1002, Some("unknown opcode")).await?;
                    anyhow::bail!("unknown opcode");
                }
            }
        }
    }

    /// 内部封装：发送任意 opcode 帧
    async fn send_frame(
        writer: &mut BufWriter<OwnedWriteHalf>,
        opcode: u8,
        payload: &[u8],
    ) -> anyhow::Result<()> {
        let mut frame = Vec::with_capacity(2 + payload.len());
        frame.push(0x80 | (opcode & 0x0f)); // FIN + opcode

        if payload.len() < 126 {
            frame.push(payload.len() as u8);
        } else if payload.len() <= 65535 {
            frame.push(126);
            frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        } else {
            frame.push(127);
            frame.extend_from_slice(&(payload.len() as u64).to_be_bytes());
        }

        frame.extend_from_slice(payload);

        writer.write_all(&frame).await?;
        writer.flush().await?;
        Ok(())
    }
    pub async fn run<'a>(
        // reader: &mut BufReader<OwnedReadHalf>,
        // writer: &mut BufWriter<OwnedWriteHalf>,
        ws: &WebSocket,
        ctx: &'a mut HTTPContext,
    ) -> anyhow::Result<()> {
        loop {
            let (opcode, payload) = {
                let mut writer_lock = ctx.writer.lock().await; // 获取 MutexGuard

                match Self::read_full(&mut ctx.reader, &mut writer_lock).await {
                    Ok(v) => v,
                    Err(_) => {
                        break;
                    }
                }
            };

            match opcode {
                0x1 => {
                    if let Some(handler) = &ws.on_text {
                        let handler = handler.clone();
                        let text = String::from_utf8_lossy(&payload).into_owned();
                        if !handler(ws, ctx, text).await {
                            let mut writer_lock = ctx.writer.lock().await; // 获取 MutexGuard
                            Self::close(&mut writer_lock, 1000, Some("handler rejected")).await?;
                            break;
                        }
                    }
                }

                0x2 => {
                    if let Some(handler) = &ws.on_binary {
                        let handler = handler.clone();
                        if !handler(ws, ctx, payload).await {
                            let mut writer_lock = ctx.writer.lock().await; // 获取 MutexGuard
                            Self::close(&mut writer_lock, 1000, Some("handler rejected")).await?;
                            break;
                        }
                    }
                }

                _ => unreachable!(),
            }
        }

        Ok(())
    }

    fn parse_close_payload(payload: &[u8]) -> anyhow::Result<(u16, Option<&str>)> {
        match payload.len() {
            0 => Ok((1000, None)),

            1 => anyhow::bail!("invalid close payload length"),

            _ => {
                let code = u16::from_be_bytes([payload[0], payload[1]]);

                // RFC 6455: 非法 close code
                match code {
                    1000 | 1001 | 1002 | 1003 | 1005 | 1006 | 1007 | 1008 | 1009 | 1010 | 1011 => {}
                    3000..=4999 => {}
                    _ => anyhow::bail!("invalid close code"),
                }

                let reason = if payload.len() > 2 {
                    let s = std::str::from_utf8(&payload[2..])
                        .map_err(|_| anyhow::anyhow!("invalid utf8 close reason"))?;
                    Some(s)
                } else {
                    None
                };

                Ok((code, reason))
            }
        }
    }

    /// 生成 WebSocket 中间件
    pub fn to_middleware(ws: WebSocket) -> Box<Executor> {
        use futures::FutureExt;
        let ws = Arc::new(Mutex::new(ws));

        Box::new(move |mut ctx: &mut HTTPContext| {
            let ws = ws.clone();
            (async move {
                let meta = &ctx.local.get_value::<HttpMetadata>().unwrap();
                if meta.method != HttpMethod::GET {
                    return true;
                }

                if !WebSocket::check(meta.method, &meta.headers) {
                    return true;
                }

                let ws = ws.lock().await;

                // 构建 WebSocket 并握手
                {
                    let mut writer_lock = ctx.writer.lock().await; // 获取 MutexGuard
                    if let Err(e) = Self::handshake(&mut writer_lock, &meta.headers).await {
                        eprintln!("WebSocket handshake failed: {:?}", e);
                        return false;
                    }
                }

                // 启动 WebSocket 循环
                if let Err(e) = Self::run(&*ws, &mut ctx).await {
                    eprintln!("WebSocket run error: {:?}", e);
                }

                false // 升级成功，不再继续 HTTP 中间件链
            })
            .boxed()
        })
    }
}
