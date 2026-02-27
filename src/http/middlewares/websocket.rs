use crate::{
    connection::context::{HTTPContext, TypeMapExt},
    http::{
        meta::HttpMetadata,
        protocol::{header::HeaderKey, method::HttpMethod},
        types::{BinaryHandler, Executor, TextHandler},
    },
};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use sha1::{Digest, Sha1};
use std::{collections::HashMap, sync::Arc};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader, BufWriter},
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

    /// 完成 WebSocket 握手 (泛型化)
    pub async fn handshake<W>(
        writer: &mut BufWriter<W>,
        headers: &HashMap<HeaderKey, String>,
    ) -> anyhow::Result<()>
    where
        W: AsyncWrite + Unpin,
    {
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

    /// 发送文本消息 (泛型化)
    pub async fn send_text<W>(writer: &mut BufWriter<W>, msg: &str) -> anyhow::Result<()>
    where
        W: AsyncWrite + Unpin,
    {
        Self::send_frame(writer, 0x1, msg.as_bytes()).await
    }

    /// 发送二进制消息 (泛型化)
    pub async fn send_binary<W>(writer: &mut BufWriter<W>, payload: &[u8]) -> anyhow::Result<()>
    where
        W: AsyncWrite + Unpin,
    {
        Self::send_frame(writer, 0x2, payload).await
    }

    /// 发送 ping (泛型化)
    pub async fn send_ping<W>(writer: &mut BufWriter<W>) -> anyhow::Result<()>
    where
        W: AsyncWrite + Unpin,
    {
        Self::send_frame(writer, 0x9, &[]).await
    }

    /// 发送 pong (泛型化)
    pub async fn send_pong<W>(writer: &mut BufWriter<W>) -> anyhow::Result<()>
    where
        W: AsyncWrite + Unpin,
    {
        Self::send_frame(writer, 0xa, &[]).await
    }

    /// 关闭连接 (泛型化)
    pub async fn close<W>(
        writer: &mut BufWriter<W>,
        code: u16,
        reason: Option<&str>,
    ) -> anyhow::Result<()>
    where
        W: AsyncWrite + Unpin,
    {
        let reason_bytes = reason.unwrap_or("").as_bytes();
        let mut payload = Vec::with_capacity(2 + reason_bytes.len());
        payload.extend_from_slice(&code.to_be_bytes());
        payload.extend_from_slice(reason_bytes);

        Self::send_frame(writer, 0x8, &payload).await?;
        writer.flush().await?;
        Ok(())
    }

    /// 读取一个完整消息 (泛型化)
    pub async fn read_full<R, W>(
        reader: &mut BufReader<R>,
        writer: &mut BufWriter<W>,
    ) -> anyhow::Result<(u8, Vec<u8>)>
    where
        R: AsyncRead + Unpin,
        W: AsyncWrite + Unpin,
    {
        loop {
            let mut header = [0u8; 2];
            reader.read_exact(&mut header).await?;

            if (header[0] & 0x70) != 0 {
                Self::close(writer, 1002, Some("RSV bits must be 0")).await?;
                anyhow::bail!("protocol error: reserved bits set");
            }

            let fin = (header[0] & 0x80) != 0;
            let opcode = header[0] & 0x0f;
            let masked = (header[1] & 0x80) != 0;
            let mut len = (header[1] & 0x7f) as usize;

            if !fin && opcode != 0x0 && opcode < 0x8 {
                // 如果不是控制帧且 fin=0，说明是分片的起始帧
                Self::close(writer, 1002, Some("fragmentation not supported")).await?;
                anyhow::bail!("fragmented frame not supported");
            }

            if opcode == 0x0 {
                // 连续帧（Continuation Frame）在没有起始帧的情况下是非法的
                Self::close(writer, 1002, Some("unexpected continuation frame")).await?;
                anyhow::bail!("protocol error: continuation frame without start");
            }

            if !masked {
                Self::close(writer, 1002, Some("unmasked frame")).await?;
                anyhow::bail!("protocol error");
            }

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
                    Self::send_pong(writer).await?;
                    continue;
                }
                0xa => continue,
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
                0x1 | 0x2 => return Ok((opcode, payload)),
                _ => {
                    Self::close(writer, 1002, Some("unknown opcode")).await?;
                    anyhow::bail!("unknown opcode");
                }
            }
        }
    }

    /// 内部封装：发送任意 opcode 帧 (泛型化)
    pub async fn send_frame<W>(
        writer: &mut BufWriter<W>,
        opcode: u8,
        payload: &[u8],
    ) -> anyhow::Result<()>
    where
        W: AsyncWrite + Unpin,
    {
        let mut frame = Vec::with_capacity(2 + payload.len());
        frame.push(0x80 | (opcode & 0x0f));

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

    /// WebSocket 运行循环 (泛型化)
    pub async fn run(ws: &WebSocket, ctx: &mut HTTPContext) -> anyhow::Result<()> {
        loop {
            let (opcode, payload) = {
                let mut writer_lock = ctx.writer.lock().await;
                match Self::read_full(&mut ctx.reader, &mut writer_lock).await {
                    Ok(v) => v,
                    Err(e) => return Err(e), // 👈 修改这里：由 break 改为 return Err(e)
                }
            };

            match opcode {
                0x1 => {
                    if let Some(handler) = &ws.on_text {
                        let handler = handler.clone();
                        let text = match String::from_utf8(payload) {
                            Ok(s) => s,
                            Err(_) => {
                                let mut writer_lock = ctx.writer.lock().await;
                                // RFC 规定：Text 帧格式错误应返回 1007
                                let _ =
                                    Self::close(&mut writer_lock, 1007, Some("invalid utf8")).await;
                                break;
                            }
                        };

                        if !handler(ws, ctx, text).await {
                            let mut writer_lock = ctx.writer.lock().await;
                            let _ =
                                Self::close(&mut writer_lock, 1000, Some("handler rejected")).await;
                            break;
                        }
                    }
                }
                0x2 => {
                    if let Some(handler) = &ws.on_binary {
                        let handler = handler.clone();
                        if !handler(ws, ctx, payload).await {
                            let mut writer_lock = ctx.writer.lock().await;
                            let _ =
                                Self::close(&mut writer_lock, 1000, Some("handler rejected")).await;
                            break;
                        }
                    }
                }
                _ => unreachable!(),
            }
        }
        Ok(())
    }

    pub fn parse_close_payload(payload: &[u8]) -> anyhow::Result<(u16, Option<&str>)> {
        match payload.len() {
            0 => Ok((1000, None)),
            1 => anyhow::bail!("invalid close payload length"),
            _ => {
                let code = u16::from_be_bytes([payload[0], payload[1]]);
                match code {
                    // 剔除 1005, 1006, 1015 等非法显式代码
                    1000 | 1001 | 1002 | 1003 | 1007 | 1008 | 1009 | 1010 | 1011 => {}
                    3000..=4999 => {}
                    _ => anyhow::bail!("invalid close code"), // 1005 现在会走到这里
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

    /// 生成 WebSocket 中间件 (泛型化支持)
    pub fn to_middleware(ws: WebSocket) -> Box<Executor> {
        use futures::FutureExt;
        let ws = Arc::new(Mutex::new(ws));

        Box::new(move |ctx: &mut HTTPContext| {
            let ws = ws.clone();
            (async move {
                let meta = ctx.local.get_value::<HttpMetadata>().unwrap();
                if meta.method != HttpMethod::GET {
                    return true;
                }

                if !WebSocket::check(meta.method, &meta.headers) {
                    return true;
                }

                let ws_guard = ws.lock().await;

                // 构建 WebSocket 并握手
                {
                    let mut writer_lock = ctx.writer.lock().await;
                    if let Err(e) = Self::handshake(&mut writer_lock, &meta.headers).await {
                        eprintln!("WebSocket handshake failed: {:?}", e);
                        return false;
                    }
                }

                // 启动 WebSocket 循环
                if let Err(e) = Self::run(&ws_guard, ctx).await {
                    eprintln!("WebSocket run error: {:?}", e);
                }

                false
            })
            .boxed()
        })
    }
}
