use std::{ io::{ self, Write }, net::SocketAddr, sync::Arc };
use tokio::net::{ TcpListener, TcpStream };
use tokio::io::{ BufReader, BufWriter };

use crate::{ router::{ Router, handle_request }, websocket::WebSocket };
use crate::handler::HTTPContext;
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
            let router = self.router.clone();

            tokio::spawn(async move {
                if let Err(err) = Self::handle_connection(router, stream, peer_addr).await {
                    eprintln!("[ERROR] Connection {}: {}", peer_addr, err);
                }
            });
        }
    }

    /// 处理 TCP 连接
    async fn handle_connection(
        router: Arc<Router>,
        stream: TcpStream,
        peer_addr: SocketAddr
    ) -> anyhow::Result<()> {
        let (reader, writer) = stream.into_split();
        let reader = BufReader::new(reader);
        let writer = BufWriter::new(writer);

        // 构建 Request
        let req = Request::new(reader, peer_addr, "").await?;
        let res = Response::new(writer);

        if !req.is_websocket {
            let mut ctx = HTTPContext {
                req,
                res,
                global: Default::default(),
                local: Default::default(),
            };

            // Trie 路由处理
            handle_request(&router, &mut ctx).await;

            // 写回响应
            let _ = ctx.res.send().await;
        } else {
            let mut ws = WebSocket {
                headers: req.headers.clone(),
                reader: req.reader,
                writer: res.writer,
            };

            // 完成握手
            ws.handshake(&req.headers).await?;

            // 开始收发消息
            while let Ok((opcode, msg)) = ws.read_full().await {
                match opcode {
                    0x1 => println!("Text: {}", String::from_utf8_lossy(&msg)),
                    0x2 => println!("Binary: {:?}", msg),
                    0x8 => {
                        ws.close(1000, Some("bye")).await?;
                        break;
                    }
                    _ => {}
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use tokio::io::{ BufReader, BufWriter, AsyncReadExt, AsyncWriteExt };
    use tokio::net::{ TcpListener, TcpStream };
    use std::net::SocketAddr;

    use crate::{
        handler::HTTPContext,
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

                    let resp_str = ctx.res.body.join("\r\n");
                    Response::write_str(&mut ctx.res.writer, &resp_str).await.unwrap();
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
                ctx.res.body.push("world".to_string());
                true
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
                ctx.res.body.push("posted".to_string());
                true
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
                // 假设 ctx.req.params.data 是 HashMap<String, String>，这里不生成新 String

                if let Some(params) = &ctx.req.params.data {
                    if let Some(id) = params.get("id") {
                        // 直接使用原始 &str
                        ctx.res.body.push(id.to_string()); // 生命周期和 params 一致
                    }
                }
                true
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
