use std::{ io::{ self, Write }, net::SocketAddr, sync::Arc };
use tokio::net::{ TcpListener, TcpStream, tcp::{ OwnedReadHalf, OwnedWriteHalf } };
use tokio::io::{ BufReader, BufWriter };

use crate::{ router::{ Router, handle_request } };
use crate::types::HTTPContext;
use crate::req::Request;
use crate::res::Response;

/// HTTPServer：无锁、并发、mut-less
pub struct HTTPServer {
    pub addr: SocketAddr,
    pub router: Arc<Router>, // Trie 路由
}

impl HTTPServer {
    pub fn new(addr: SocketAddr, router: Router) -> Self {
        Self {
            addr,
            router: Arc::new(router),
        }
    }

    /// 启动服务器
    pub async fn run(&self) -> std::io::Result<()> {
        let listener = TcpListener::bind(self.addr).await?;
        println!("HTTPServer listening on {}", self.addr);
        io::stdout().flush().unwrap();

        loop {
            let (stream, peer_addr) = listener.accept().await?;
            let (mut reader, writer) = stream.into_split();
            if Request::is_http_connection(&mut reader).await.unwrap() {
                let router = self.router.clone();

                tokio::spawn(async move {
                    let reader = BufReader::new(reader);
                    let writer = BufWriter::new(writer);

                    if
                        let Err(err) = Self::handle_connection(
                            router,
                            reader,
                            writer,
                            peer_addr
                        ).await
                    {
                        eprintln!("[ERROR] Connection {}: {}", peer_addr, err);
                    }
                });
            }
        }
    }

    /// 处理 TCP 连接
    async fn handle_connection(
        router: Arc<Router>,
        reader: BufReader<OwnedReadHalf>,
        writer: BufWriter<OwnedWriteHalf>,
        peer_addr: SocketAddr
    ) -> anyhow::Result<()> {
        // 构建 Request
        let req = Request::new(reader, peer_addr, "").await?;
        let res = Response::new(writer);
        let mut ctx = HTTPContext {
            req,
            res,
            global: Default::default(),
            local: Default::default(),
        };

        // 如果返回true启动默认处理机制，即统一发送body与header。
        if handle_request(&router, &mut ctx).await {
            // 写回响应
            let _ = ctx.res.send().await;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use futures::FutureExt;
    use tokio::io::{ BufReader, BufWriter, AsyncReadExt, AsyncWriteExt };
    use tokio::net::{ TcpListener, TcpStream };
    use std::net::SocketAddr;

    use crate::{
        types::HTTPContext,
        res::Response,
        req::Request,
        router::{ Router, NodeType, handle_request },
    };

    /// 简单帮助函数：生成 HTTPServer 并在 background 运行
    async fn spawn_server(root: Router) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let router = Arc::new(root);

        tokio::spawn(async move {
            loop {
                let (stream, peer_addr) = listener.accept().await.unwrap();
                let router = router.clone();
                tokio::spawn(async move {
                    let (reader, writer) = stream.into_split();
                    let reader = BufReader::new(reader);
                    let writer = BufWriter::new(writer);

                    let req = Request::new(reader, peer_addr, "").await;
                    let res = Response::new(writer);

                    let mut ctx = HTTPContext {
                        req: req.expect("Request is illegal!"),
                        res,
                        global: Default::default(),
                        local: Default::default(),
                    };

                    handle_request(&router, &mut ctx).await;
                    ctx.res.send().await
                });
            }
        });

        addr
    }

    #[tokio::test]
    async fn test_trie_server_get() {
        // 构建 Trie
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
            None
        );

        // 启动 HTTPServer
        let addr = spawn_server(root).await;

        // 客户端请求
        let mut client = TcpStream::connect(addr).await.unwrap();
        client.write_all(b"GET /hello HTTP/1.1\r\nHost: x\r\n\r\n").await.unwrap();

        let mut resp = vec![0; 1024];
        let n = client.read(&mut resp).await.unwrap();
        let resp_str = std::str::from_utf8(&resp[..n]).unwrap();

        assert!(resp_str.contains("world"));
    }

    #[tokio::test]
    async fn test_trie_server_post() {
        // 构建 Trie
        let mut root = Router::new(NodeType::Static("root".into()));
        root.insert(
            "/user/:id/profile",
            Some("POST"),
            Arc::new(|ctx| {
                Box::pin(async move {
                    ctx.res.body.push("posted".to_string());
                    true
                }).boxed()
            }),
            None
        );

        let addr = spawn_server(root).await;

        // 客户端请求
        let mut client = TcpStream::connect(addr).await.unwrap();
        client.write_all(b"POST /user/abc/profile HTTP/1.1\r\nHost: x\r\n\r\n").await.unwrap();

        let mut resp = vec![0; 1024];
        let n = client.read(&mut resp).await.unwrap();
        let resp_str = std::str::from_utf8(&resp[..n]).unwrap();

        assert!(resp_str.contains("posted"));
    }

    #[tokio::test]
    async fn test_trie_server_dynamic_param() {
        let mut root = Router::new(NodeType::Static("root".into()));

        root.insert(
            "/user/:id",
            Some("GET"),
            Arc::new(|ctx| {
                Box::pin(async move {
                    // 假设 ctx.req.params.data 是 HashMap<String, String>，这里不生成新 String
                    if let Some(params) = &ctx.req.params.data {
                        if let Some(id) = params.get("id") {
                            // 直接使用原始 &str
                            ctx.res.body.push(id.to_string()); // 生命周期和 params 一致
                        }
                    }
                    true
                }).boxed()
            }),
            None
        );

        let addr = spawn_server(root).await;

        let mut client = TcpStream::connect(addr).await.unwrap();
        client.write_all(b"GET /user/42 HTTP/1.1\r\nHost: x\r\n\r\n").await.unwrap();

        let mut resp = vec![0; 1024];
        let n = client.read(&mut resp).await.unwrap();
        let resp_str = std::str::from_utf8(&resp[..n]).unwrap();

        assert!(resp_str.contains("42"));
    }
}

#[cfg(test)]
mod tcp_macro_tests {
    use super::*;
    use crate::{ get, route };
    use crate::types::{ HTTPContext };
    use crate::router::{ Router, NodeType };
    use crate::websocket::WebSocket;
    use futures::FutureExt;
    use std::sync::Arc;
    use tokio::io::{ BufReader, BufWriter, AsyncReadExt, AsyncWriteExt };
    use tokio::net::{ TcpListener, TcpStream };

    async fn setup_server() -> (TcpListener, u16) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        (listener, port)
    }

    async fn create_client(port: u16) -> TcpStream {
        TcpStream::connect(("127.0.0.1", port)).await.unwrap()
    }

    #[tokio::test]
    async fn test_tcp_macro_ws() {
        // 准备 Router
        let mut root = Router::new(NodeType::Static("root".into()));

        // WebSocket 中间件
        let ws_middleware = WebSocket {
            on_text: Some(
                Arc::new(|_ws, _ctx, msg| {
                    (
                        async move {
                            assert_eq!(msg, "ping");
                            true
                        }
                    ).boxed()
                })
            ),
            on_binary: None,
        };

        // 使用宏注册路由并添加 ws_middleware
        route!(
            root,
            get!(
                "/hello",
                |ctx: &mut HTTPContext| {
                    (
                        async move {
                            ctx.res.body.push("HTTP GET".into());
                            true
                        }
                    ).boxed()
                },
                [WebSocket::to_middleware(ws_middleware)]
            )
        );

        // 启动 TCP 服务
        let (listener, port) = setup_server().await;
        let server_task = tokio::spawn(async move {
            let (stream, peer_addr) = listener.accept().await.unwrap();
            let (reader, writer) = stream.into_split();
            let reader = BufReader::new(reader);
            let writer = BufWriter::new(writer);

            // 构建 HTTPContext
            let req = Request::new(reader, peer_addr, "").await.unwrap();
            let res = Response::new(writer);
            let mut ctx = HTTPContext {
                req,
                res,
                global: Default::default(),
                local: Default::default(),
            };

            handle_request(&root, &mut ctx).await;

            // 写回响应
            let _ = ctx.res.send().await;
        });

        // 启动客户端
        let client_task = tokio::spawn(async move {
            let mut stream = create_client(port).await;
            let (reader, writer) = stream.split();
            let mut buf_reader = BufReader::new(reader);
            let mut buf_writer = BufWriter::new(writer);

            // 发送 HTTP GET /hello
            let req =
                "GET /hello HTTP/1.1\r\nHost: 127.0.0.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n";
            buf_writer.write_all(req.as_bytes()).await.unwrap();
            buf_writer.flush().await.unwrap();

            // 读取服务端 handshake
            let mut buf = vec![0u8; 1024];
            let n = buf_reader.read(&mut buf).await.unwrap();
            let resp = String::from_utf8_lossy(&buf[..n]);
            assert!(resp.contains("101 Switching Protocols"));

            // 发送一个 masked text frame "ping"
            let payload = b"ping";
            let mask = [1, 2, 3, 4];
            let mut frame = vec![0x81, 0x80 | (payload.len() as u8)];
            frame.extend_from_slice(&mask);
            frame.extend(
                payload
                    .iter()
                    .enumerate()
                    .map(|(i, b)| b ^ mask[i % 4])
                    .collect::<Vec<_>>()
            );
            buf_writer.write_all(&frame).await.unwrap();
            buf_writer.flush().await.unwrap();
        });

        tokio::join!(server_task, client_task);
    }
}
