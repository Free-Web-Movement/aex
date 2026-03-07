use crate::{
    connection::context::{ Context, TypeMapExt },
    http::{
        meta::HttpMetadata,
        protocol::{ header::HeaderKey, method::HttpMethod },
        types::{ Executor },
        websocket::{ WSCodec, WSFrame, WebSocketHandler },
    },
    // 假设这些是你定义的路径
};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use sha1::{ Digest, Sha1 };
use std::{ collections::HashMap, sync::Arc, pin::Pin, task::{ Poll, Context as TaskContext } };
use tokio::{ io::{ AsyncRead, AsyncWrite, AsyncWriteExt, ReadBuf } };
use tokio_util::codec::Framed;
use futures::{ SinkExt, StreamExt, FutureExt };

use futures::future::BoxFuture;

/// 用于组合 Context 中的 reader 和 writer
struct CombinedStream {
    reader: Box<dyn tokio::io::AsyncRead + Send + Unpin>,
    writer: Box<dyn tokio::io::AsyncWrite + Send + Unpin>,
}

impl AsyncRead for CombinedStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &mut ReadBuf<'_>
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.reader).poll_read(cx, buf)
    }
}

impl AsyncWrite for CombinedStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &[u8]
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.writer).poll_write(cx, buf)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.writer).poll_flush(cx)
    }
    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.writer).poll_shutdown(cx)
    }
}

pub struct WebSocket {
    pub on_frame: Option<WebSocketHandler>,
}

impl WebSocket {
    pub fn new() -> Self {
        Self { on_frame: None }
    }

    /// 设置通用处理器
    pub fn set_handler<F>(mut self, handler: F) -> Self
        where
            F: Fn(&WebSocket, &mut Context, WSFrame) -> BoxFuture<'static, bool> +
                Send +
                Sync +
                'static
    {
        self.on_frame = Some(Arc::new(handler));
        self
    }

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
        writer: &mut (dyn AsyncWrite + Send + Unpin),
        headers: &HashMap<HeaderKey, String>
    ) -> anyhow::Result<()> {
        let key = headers
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

        writer.write_all(response.as_bytes()).await?;
        writer.flush().await?;
        Ok(())
    }

    /// WebSocket 核心运行循环
    pub async fn run(ws: &WebSocket, ctx: &mut Context) -> anyhow::Result<()> {
        let reader = ctx.reader.take().ok_or_else(|| anyhow::anyhow!("Reader missing"))?;
        let writer = ctx.writer.take().ok_or_else(|| anyhow::anyhow!("Writer missing"))?;

        let io = CombinedStream { reader, writer };
        let mut framed = Framed::new(io, WSCodec);

        while let Some(result) = framed.next().await {
            let frame = match result {
                Ok(f) => f,
                Err(e) => {
                    return Err(anyhow::anyhow!("Protocol error: {}", e));
                }
            };

            // 检查是否有通用处理器
            if let Some(handler) = &ws.on_frame {
                // 调用处理器。如果返回 false，表示业务层要求关闭连接
                if !handler(ws, ctx, frame.clone()).await {
                    let _ = framed.send(WSFrame::Close(1000, Some("Handler exit".into()))).await;
                    break;
                }
            }

            // 默认行为处理 (如果 Handler 没拦截或者没设置，可以在这里做兜底)
            match frame {
                WSFrame::Ping(p) => {
                    framed.send(WSFrame::Pong(p)).await?;
                }
                WSFrame::Close(code, reason) => {
                    let _ = framed.send(WSFrame::Close(code, reason)).await;
                    break;
                }
                _ => {} // Text/Binary 在没有设置 Handler 时默认忽略
            }
        }
        Ok(())
    }

    /// 生成 WebSocket 中件间
    pub fn to_middleware(ws: WebSocket) -> Box<Executor> {
        let ws = Arc::new(ws);

        Box::new(move |ctx: &mut Context| {
            let ws = ws.clone();
            (
                async move {
                    let meta = match ctx.local.get_value::<HttpMetadata>() {
                        Some(m) => m,
                        None => {
                            return true;
                        }
                    };

                    if !Self::check(meta.method, &meta.headers) {
                        return true;
                    }

                    // 进行握手
                    {
                        let w = ctx.writer.as_deref_mut().unwrap();
                        if let Err(e) = Self::handshake(w, &meta.headers).await {
                            eprintln!("WS Handshake Error: {:?}", e);
                            return false;
                        }
                    }

                    // 启动循环 (内部会接管 reader/writer)
                    if let Err(e) = Self::run(&ws, ctx).await {
                        eprintln!("WS Connection Ended: {:?}", e);
                    }

                    false // 拦截，不继续执行后续 HTTP 中间件
                }
            ).boxed()
        })
    }

    /// 严格按照 RFC 6455 解析 Close 帧负载，返回借用的 &str 以优化性能
    pub fn parse_close_payload(payload: &[u8]) -> anyhow::Result<(u16, Option<&str>)> {
        let len = payload.len();

        // 1. 空负载：协议规定视为 1005 (No Status Rcvd)
        if len == 0 {
            return Ok((1005, None));
        }

        // 2. 异常长度：如果有载荷但不足 2 字节，属于协议错误
        if len < 2 {
            anyhow::bail!("Incomplete close status code");
        }

        // 3. 提取状态码 (Big-Endian)
        let code = u16::from_be_bytes([payload[0], payload[1]]);

        // 4. 解析原因 (必须是有效的 UTF-8)
        let reason = if len > 2 {
            let s = std::str::from_utf8(&payload[2..])?;
            Some(s)
        } else {
            None
        };

        Ok((code, reason))
    }
}
