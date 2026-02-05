use std::collections::HashMap;
use tokio::{
    io::{ AsyncReadExt, AsyncWriteExt, BufReader, BufWriter },
    net::tcp::{ OwnedReadHalf, OwnedWriteHalf },
};
use crate::protocol::{ header::HeaderKey, method::HttpMethod };
use sha1::{ Sha1, Digest };
use base64::engine::general_purpose::STANDARD;
use base64::Engine;

pub struct WebSocket {
    pub headers: HashMap<HeaderKey, String>,
    pub reader: BufReader<OwnedReadHalf>,
    pub writer: BufWriter<OwnedWriteHalf>,
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
        &mut self,
        req_headers: &HashMap<HeaderKey, String>
    ) -> anyhow::Result<()> {
        let key = req_headers
            .get(&HeaderKey::SecWebSocketKey)
            .ok_or_else(|| anyhow::anyhow!("missing Sec-WebSocket-Key"))?;

        let mut sha = Sha1::new();
        sha.update(key.as_bytes());
        sha.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
        let accept_key = STANDARD.encode(sha.finalize());

        let response =
            format!("HTTP/1.1 101 Switching Protocols\r\n\
            Upgrade: websocket\r\n\
            Connection: Upgrade\r\n\
            Sec-WebSocket-Accept: {}\r\n\r\n", accept_key);

        self.writer.write_all(response.as_bytes()).await?;
        self.writer.flush().await?;
        Ok(())
    }

    /// 发送文本消息
    pub async fn send_text(&mut self, msg: &str) -> anyhow::Result<()> {
        self.send_frame(0x1, msg.as_bytes()).await
    }

    /// 发送二进制消息
    pub async fn send_binary(&mut self, payload: &[u8]) -> anyhow::Result<()> {
        self.send_frame(0x2, payload).await
    }

    /// 发送 ping
    pub async fn send_ping(&mut self) -> anyhow::Result<()> {
        self.send_frame(0x9, &[]).await
    }

    /// 发送 pong
    pub async fn send_pong(&mut self) -> anyhow::Result<()> {
        self.send_frame(0xa, &[]).await
    }

    /// 关闭连接
    pub async fn close(&mut self, code: u16, reason: Option<&str>) -> anyhow::Result<()> {
        let reason_bytes = reason.unwrap_or("").as_bytes();
        let mut payload = Vec::with_capacity(2 + reason_bytes.len());
        payload.extend_from_slice(&code.to_be_bytes());
        payload.extend_from_slice(reason_bytes);

        self.send_frame(0x8, &payload).await?;
        self.writer.flush().await?;
        Ok(())
    }

    /// 读取一个完整消息，返回 (opcode, payload)
    /// 自动处理 ping/pong
    pub async fn read_full(&mut self) -> anyhow::Result<(u8, Vec<u8>)> {
        loop {
            let mut header = [0u8; 2];
            self.reader.read_exact(&mut header).await?;

            let fin = (header[0] & 0x80) != 0;
            let opcode = header[0] & 0x0f;
            let masked = (header[1] & 0x80) != 0;
            let mut payload_len = (header[1] & 0x7f) as usize;

            if payload_len == 126 {
                let mut buf = [0u8; 2];
                self.reader.read_exact(&mut buf).await?;
                payload_len = u16::from_be_bytes(buf) as usize;
            } else if payload_len == 127 {
                let mut buf = [0u8; 8];
                self.reader.read_exact(&mut buf).await?;
                payload_len = u64::from_be_bytes(buf) as usize;
            }

            let mut mask = [0u8; 4];
            if masked {
                self.reader.read_exact(&mut mask).await?;
            }

            let mut payload = vec![0u8; payload_len];
            self.reader.read_exact(&mut payload).await?;

            if masked {
                for i in 0..payload_len {
                    payload[i] ^= mask[i % 4];
                }
            }

            // 自动处理 ping/pong
            match opcode {
                0x9 => {
                    self.send_pong().await?;
                    continue;
                } // ping -> pong 自动应答
                0xa => {
                    continue;
                } // pong -> 忽略
                _ => {
                    return Ok((opcode, payload));
                } // 文本/二进制/关闭/其它
            }
        }
    }

    /// 内部封装：发送任意 opcode 帧
    async fn send_frame(&mut self, opcode: u8, payload: &[u8]) -> anyhow::Result<()> {
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

        self.writer.write_all(&frame).await?;
        self.writer.flush().await?;
        Ok(())
    }
}
