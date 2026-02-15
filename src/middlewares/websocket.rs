use std::{ collections::HashMap, sync::Arc };
use tokio::{
    io::{ AsyncReadExt, AsyncWriteExt, BufReader, BufWriter },
    net::tcp::{ OwnedReadHalf, OwnedWriteHalf },
    sync::Mutex,
};
use crate::{
    protocol::{ header::HeaderKey, method::HttpMethod },
    types::{ BinaryHandler, Executor, HTTPContext, TextHandler },
};
use sha1::{ Sha1, Digest };
use base64::engine::general_purpose::STANDARD;
use base64::Engine;

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

    /// 发送文本消息
    pub async fn send_text(
        writer: &mut BufWriter<OwnedWriteHalf>,
        msg: &str
    ) -> anyhow::Result<()> {
        Self::send_frame(writer, 0x1, msg.as_bytes()).await
    }

    /// 发送二进制消息
    pub async fn send_binary(
        writer: &mut BufWriter<OwnedWriteHalf>,
        payload: &[u8]
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
        reason: Option<&str>
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
        writer: &mut BufWriter<OwnedWriteHalf>
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
        payload: &[u8]
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
        ctx: &'a mut HTTPContext
    ) -> anyhow::Result<()> {
        loop {
            let (opcode, payload) = match
                Self::read_full(&mut ctx.req.reader, &mut ctx.res.writer).await
            {
                Ok(v) => v,
                Err(_) => {
                    break;
                }
            };

            match opcode {
                0x1 => {
                    if let Some(handler) = &ws.on_text {
                        let handler = handler.clone();
                        let text = String::from_utf8_lossy(&payload).into_owned();
                        if !handler(ws, ctx, text).await {
                            Self::close(&mut ctx.res.writer, 1000, Some("handler rejected")).await?;
                            break;
                        }
                    }
                }

                0x2 => {
                    if let Some(handler) = &ws.on_binary {
                        let handler = handler.clone();
                        if !handler(ws, ctx, payload).await {
                            Self::close(&mut ctx.res.writer, 1000, Some("handler rejected")).await?;
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
                    let s = std::str
                        ::from_utf8(&payload[2..])
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
            (
                async move {
                    if ctx.req.method != HttpMethod::GET {
                        return true;
                    }

                    if !WebSocket::check(ctx.req.method, &ctx.req.headers) {
                        return true;
                    }

                    let ws = ws.lock().await;

                    // 构建 WebSocket 并握手
                    if let Err(e) = Self::handshake(&mut ctx.res.writer, &ctx.req.headers).await {
                        eprintln!("WebSocket handshake failed: {:?}", e);
                        return false;
                    }

                    // 启动 WebSocket 循环
                    if let Err(e) = Self::run(&*ws, &mut ctx).await {
                        eprintln!("WebSocket run error: {:?}", e);
                    }

                    false // 升级成功，不再继续 HTTP 中间件链
                }
            ).boxed()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::header::HeaderKey;
    use crate::req::Request;
    use crate::res::Response;
    use crate::types::{ HTTPContext, TypeMap };
    use tokio::net::{ TcpListener, TcpStream };
    use tokio::io::{ BufReader, BufWriter, AsyncReadExt, AsyncWriteExt };
    use std::collections::HashMap;
    use std::sync::Arc;
    use futures::FutureExt;

    async fn setup_server() -> (TcpListener, u16) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        (listener, port)
    }

    async fn create_client(port: u16) -> TcpStream {
        TcpStream::connect(("127.0.0.1", port)).await.unwrap()
    }

    #[tokio::test]
    async fn test_websocket_tcp_flow() {
        let (listener, port) = setup_server().await;

        let server_task = tokio::spawn(async move {
            let (stream, peer_addr) = listener.accept().await.unwrap();
            let (reader, writer) = stream.into_split();
            let reader = BufReader::new(reader);
            let writer = BufWriter::new(writer);

            // 构造 HTTPContext
            let mut headers: HashMap<HeaderKey, String> = HashMap::new();
            headers.insert(HeaderKey::Upgrade, "websocket".into());
            headers.insert(HeaderKey::Connection, "Upgrade".into());
            headers.insert(HeaderKey::SecWebSocketKey, "dGhlIHNhbXBsZSBub25jZQ==".into());

            // 构建 Request
            let req = Request::new(reader, peer_addr, "").await.unwrap();
            let res = Response::new(writer);
            let mut ctx = HTTPContext {
                req,
                res,
                global: Arc::new(Mutex::new(TypeMap::new())),
                local: TypeMap::new(),
            };

            let ws = WebSocket {
                on_text: Some(
                    Arc::new(|_ws, _ctx, msg|
                        (
                            async move {
                                assert_eq!(msg, "hello from client");
                                true
                            }
                        ).boxed()
                    )
                ),
                on_binary: Some(
                    Arc::new(|_ws, _ctx, payload|
                        (
                            async move {
                                assert_eq!(payload, b"binary data");
                                true
                            }
                        ).boxed()
                    )
                ),
            };

            // 执行 handshake
            WebSocket::handshake(&mut ctx.res.writer, &ctx.req.headers).await.unwrap();

            // 接收客户端消息并运行 run
            WebSocket::run(&ws, &mut ctx).await.unwrap();
        });

        let client_task = tokio::spawn(async move {
            let stream = create_client(port).await;
            let (reader, writer) = stream.into_split();
            let mut reader = BufReader::new(reader);
            let mut writer = BufWriter::new(writer);
            // 模拟 WebSocket 客户端发送 handshake
            let handshake =
                "\
                GET / HTTP/1.1\r\n\
                Host: 127.0.0.1\r\n\
                Upgrade: websocket\r\n\
                Connection: Upgrade\r\n\
                Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
                Sec-WebSocket-Version: 13\r\n\r\n";
            writer.write_all(handshake.as_bytes()).await.unwrap();
            writer.flush().await.unwrap();

            // 读取服务器 handshake 响应
            let mut buf = vec![0u8; 1024];
            let n = reader.read(&mut buf).await.unwrap();
            assert!(String::from_utf8_lossy(&buf[..n]).contains("101 Switching Protocols"));

            // 发送 masked text frame
            let text = b"hello from client";
            let mask = [1, 2, 3, 4];
            let mut frame = vec![0x81, 0x80 | (text.len() as u8)]; // FIN + opcode, masked + len
            frame.extend_from_slice(&mask);
            frame.extend(
                text
                    .iter()
                    .enumerate()
                    .map(|(i, b)| b ^ mask[i % 4])
                    .collect::<Vec<_>>()
            );
            writer.write_all(&frame).await.unwrap();
            writer.flush().await.unwrap();

            // 发送 masked binary frame
            let payload = b"binary data";
            let mask = [4, 3, 2, 1];
            let mut frame = vec![0x82, 0x80 | (payload.len() as u8)];
            frame.extend_from_slice(&mask);
            frame.extend(
                payload
                    .iter()
                    .enumerate()
                    .map(|(i, b)| b ^ mask[i % 4])
                    .collect::<Vec<_>>()
            );
            writer.write_all(&frame).await.unwrap();
            writer.flush().await.unwrap();
        });

        tokio::join!(server_task, client_task);
    }

    #[tokio::test]
    async fn test_websocket_full_tcp_flow() {
        let (listener, port) = setup_server().await;

        let server_task = tokio::spawn(async move {
            let (stream, peer_addr) = listener.accept().await.unwrap();
            let (reader, writer) = stream.into_split();
            let reader = BufReader::new(reader);
            let writer = BufWriter::new(writer);

            let req = Request::new(reader, peer_addr, "").await.unwrap();
            let res = Response::new(writer);
            let mut ctx = HTTPContext {
                req,
                res,
                global: Arc::new(Mutex::new(TypeMap::new())),
                local: TypeMap::new(),
            };

            let ws = WebSocket {
                on_text: Some(
                    Arc::new(|_ws, _ctx, msg|
                        (
                            async move {
                                assert_eq!(msg, "hello from client");
                                true
                            }
                        ).boxed()
                    )
                ),
                on_binary: Some(
                    Arc::new(|_ws, _ctx, payload|
                        (
                            async move {
                                assert_eq!(payload, b"binary data");
                                true
                            }
                        ).boxed()
                    )
                ),
            };

            // handshake
            WebSocket::handshake(&mut ctx.res.writer, &ctx.req.headers).await.unwrap();

            // 测试 run，同时触发 ping/pong、text、binary
            WebSocket::run(&ws, &mut ctx).await.unwrap();
        });

        let client_task = tokio::spawn(async move {
            let mut stream = create_client(port).await;
            let (reader, writer) = stream.into_split();
            let mut reader = BufReader::new(reader);
            let mut writer = BufWriter::new(writer);

            // handshake
            let handshake =
                "\
                GET / HTTP/1.1\r\n\
                Host: 127.0.0.1\r\n\
                Upgrade: websocket\r\n\
                Connection: Upgrade\r\n\
                Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
                Sec-WebSocket-Version: 13\r\n\r\n";
            writer.write_all(handshake.as_bytes()).await.unwrap();
            writer.flush().await.unwrap();

            let mut buf = vec![0u8; 1024];
            let n = reader.read(&mut buf).await.unwrap();
            assert!(String::from_utf8_lossy(&buf[..n]).contains("101 Switching Protocols"));

            // 发送 masked text frame
            let text = b"hello from client";
            let mask = [1, 2, 3, 4];
            let mut frame = vec![0x81, 0x80 | (text.len() as u8)];
            frame.extend_from_slice(&mask);
            frame.extend(
                text
                    .iter()
                    .enumerate()
                    .map(|(i, b)| b ^ mask[i % 4])
                    .collect::<Vec<_>>()
            );
            writer.write_all(&frame).await.unwrap();
            writer.flush().await.unwrap();

            // 发送 masked binary frame
            let payload = b"binary data";
            let mask = [4, 3, 2, 1];
            let mut frame = vec![0x82, 0x80 | (payload.len() as u8)];
            frame.extend_from_slice(&mask);
            frame.extend(
                payload
                    .iter()
                    .enumerate()
                    .map(|(i, b)| b ^ mask[i % 4])
                    .collect::<Vec<_>>()
            );
            writer.write_all(&frame).await.unwrap();
            writer.flush().await.unwrap();

            // 发送 ping frame, server 会自动 pong
            let mask = [1, 1, 1, 1];
            let mut frame = vec![0x89, 0x80]; // FIN+opcode=9, masked+len=0
            frame.extend_from_slice(&mask);
            writer.write_all(&frame).await.unwrap();
            writer.flush().await.unwrap();

            // 发送 close frame, server 处理 close 分支
            let code: u16 = 1000;
            let reason = b"client close";
            let mask = [1, 2, 3, 4];
            let mut payload = Vec::new();
            payload.extend_from_slice(&code.to_be_bytes());
            payload.extend_from_slice(reason);
            let mut frame = vec![0x88, 0x80 | (payload.len() as u8)];
            frame.extend_from_slice(&mask);
            frame.extend(
                payload
                    .iter()
                    .enumerate()
                    .map(|(i, b)| b ^ mask[i % 4])
                    .collect::<Vec<_>>()
            );
            writer.write_all(&frame).await.unwrap();
            writer.flush().await.unwrap();

            // 测试 protocol error: 发送 unmasked frame
            let mut frame = vec![0x81, text.len() as u8]; // FIN+text, unmasked
            frame.extend_from_slice(text);
            let _ = writer.write_all(&frame).await; // 允许错误被 server 捕获
            writer.flush().await.unwrap();
        });

        tokio::join!(server_task, client_task);
    }

    #[tokio::test]
    async fn test_websocket_edge_cases() {
        let (listener, port) = setup_server().await;

        let server_task = tokio::spawn(async move {
            let (stream, peer_addr) = listener.accept().await.unwrap();
            let (reader, writer) = stream.into_split();
            let reader = BufReader::new(reader);
            let writer = BufWriter::new(writer);

            let req = Request::new(reader, peer_addr, "").await.unwrap();
            let res = Response::new(writer);
            let mut ctx = HTTPContext {
                req,
                res,
                global: Arc::new(Mutex::new(TypeMap::new())),
                local: TypeMap::new(),
            };

            let ws = WebSocket {
                on_text: None,
                on_binary: None,
            };

            // handshake
            WebSocket::handshake(&mut ctx.res.writer, &ctx.req.headers).await.unwrap();

            // run 触发 fragmentation / unknown opcode / control frame > 125 / unmasked
            let _ = WebSocket::run(&ws, &mut ctx).await;
        });

        let client_task = tokio::spawn(async move {
            let mut stream = create_client(port).await;
            let (reader, writer) = stream.into_split();
            let mut writer = BufWriter::new(writer);

            // handshake
            let handshake =
                "\
            GET / HTTP/1.1\r\n\
            Host: 127.0.0.1\r\n\
            Upgrade: websocket\r\n\
            Connection: Upgrade\r\n\
            Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
            Sec-WebSocket-Version: 13\r\n\r\n";
            writer.write_all(handshake.as_bytes()).await.unwrap();
            writer.flush().await.unwrap();

            // 1️⃣ fragmentation (fin=0, opcode=1)
            let mask = [1, 2, 3, 4];
            let mut frame = vec![0x01, 0x80]; // fin=0
            frame.extend_from_slice(&mask);
            frame.extend_from_slice(&[b'a' ^ mask[0]]);
            writer.write_all(&frame).await.unwrap();
            writer.flush().await.unwrap();

            // 2️⃣ control frame payload > 125 (ping frame)
            let mask = [1, 1, 1, 1];
            let mut payload = vec![0u8; 126]; // 超长 payload
            let mut frame = vec![0x89, 0x80 | 126]; // opcode=9, masked=1, len=126
            frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
            frame.extend_from_slice(&mask);
            for i in 0..payload.len() {
                payload[i] ^= mask[i % 4];
            }
            frame.extend_from_slice(&payload);
            let _ = writer.write_all(&frame).await; // server 会 close
            writer.flush().await.unwrap();

            // 3️⃣ unknown opcode (0xB)
            let mask = [1, 2, 3, 4];
            let payload = b"unknown";
            let mut frame = vec![0x8b, 0x80 | (payload.len() as u8)]; // FIN + opcode=0xB, masked
            frame.extend_from_slice(&mask);
            frame.extend(
                payload
                    .iter()
                    .enumerate()
                    .map(|(i, b)| b ^ mask[i % 4])
                    .collect::<Vec<_>>()
            );
            let _ = writer.write_all(&frame).await;
            writer.flush().await.unwrap();
        });

        tokio::join!(server_task, client_task);
    }

    #[cfg(test)]
    mod websocket_router_test {
        use super::*;
        use crate::types::{ HTTPContext };
        use crate::req::Request;
        use crate::res::Response;
        use tokio::net::{ TcpListener, TcpStream };
        use tokio::io::{ BufReader, BufWriter, AsyncReadExt, AsyncWriteExt };
        use std::sync::Arc;
        use futures::FutureExt;

        /// 假设你的 Router 支持 NodeType 和 insert 方法
        use crate::router::{ NodeType, Router, handle_request };

        /// TCP server setup
        async fn setup_server() -> (TcpListener, u16) {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            (listener, port)
        }

        /// TCP client connect
        async fn create_client(port: u16) -> TcpStream {
            TcpStream::connect(("127.0.0.1", port)).await.unwrap()
        }

        #[tokio::test]
        async fn test_ws_middleware_in_router_node() {
            let (listener, port) = setup_server().await;

            // WebSocket 处理逻辑
            let ws = WebSocket {
                on_text: Some(
                    Arc::new(|_ws, _ctx, msg|
                        (
                            async move {
                                assert_eq!(msg, "hello client");
                                true
                            }
                        ).boxed()
                    )
                ),
                on_binary: Some(
                    Arc::new(|_ws, _ctx, payload|
                        (
                            async move {
                                assert_eq!(payload, b"binary payload");
                                true
                            }
                        ).boxed()
                    )
                ),
            };

            let ws_middleware = WebSocket::to_middleware(ws);

            // 构建 Trie 路由
            let mut root = Router::new(NodeType::Static("root".into()));
            root.insert(
                "/hello",
                Some("GET"),
                Arc::new(|ctx| {
                    Box::pin(async move {
                        ctx.res.body.push("world".to_string());
                        true
                    }).boxed()
                }),
                Some(vec![Arc::from(ws_middleware)]) // 传入 WebSocket 中间件
            );

            // Server task
            let server_task = tokio::spawn(async move {
                let (stream, peer_addr) = listener.accept().await.unwrap();
                let (reader, writer) = stream.into_split();
                let reader = BufReader::new(reader);
                let writer = BufWriter::new(writer);

                let req = Request::new(reader, peer_addr, "").await.unwrap();
                let res = Response::new(writer);
                let mut ctx = HTTPContext {
                    req,
                    res,
                global: Arc::new(Mutex::new(TypeMap::new())),
                local: TypeMap::new(),
                };

                handle_request(&root, &mut ctx).await;

                // 写回响应
                let _ = ctx.res.send().await;
                // root.insert("/hello", Some("GET"), &mut ctx).await.unwrap();
            });

            // Client task: 发起 WebSocket handshake + 发送消息
            let client_task = tokio::spawn(async move {
                let stream = create_client(port).await;
                let (reader, writer) = stream.into_split();
                let mut reader = BufReader::new(reader);
                let mut writer = BufWriter::new(writer);

                // WebSocket handshake 请求
                let handshake =
                    "\
                GET /hello HTTP/1.1\r\n\
                Host: 127.0.0.1\r\n\
                Upgrade: websocket\r\n\
                Connection: Upgrade\r\n\
                Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
                Sec-WebSocket-Version: 13\r\n\r\n";
                writer.write_all(handshake.as_bytes()).await.unwrap();
                writer.flush().await.unwrap();

                // 读取服务器 handshake 响应
                let mut buf = vec![0u8; 1024];
                let n = reader.read(&mut buf).await.unwrap();
                let resp_str = String::from_utf8_lossy(&buf[..n]);
                assert!(resp_str.contains("101 Switching Protocols"));

                // 发送 masked text frame
                let text = b"hello client";
                let mask = [1, 2, 3, 4];
                let mut frame = vec![0x81, 0x80 | (text.len() as u8)]; // FIN + text opcode, masked
                frame.extend_from_slice(&mask);
                frame.extend(
                    text
                        .iter()
                        .enumerate()
                        .map(|(i, b)| b ^ mask[i % 4])
                        .collect::<Vec<_>>()
                );
                writer.write_all(&frame).await.unwrap();
                writer.flush().await.unwrap();

                // 发送 masked binary frame
                let payload = b"binary payload";
                let mask = [4, 3, 2, 1];
                let mut frame = vec![0x82, 0x80 | (payload.len() as u8)]; // FIN + binary opcode, masked
                frame.extend_from_slice(&mask);
                frame.extend(
                    payload
                        .iter()
                        .enumerate()
                        .map(|(i, b)| b ^ mask[i % 4])
                        .collect::<Vec<_>>()
                );
                writer.write_all(&frame).await.unwrap();
                writer.flush().await.unwrap();
            });

            tokio::join!(server_task, client_task);
        }
    }
}
