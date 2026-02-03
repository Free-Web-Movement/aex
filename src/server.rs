use std::{ io::{ self, Write }, net::SocketAddr, sync::Arc };
use tokio::net::{ TcpListener, TcpStream };
use tokio::io::{ BufReader, BufWriter };

use crate::trie::{ TrieNode, handle_request };
use crate::handler::HTTPContext;
use crate::req::Request;
use crate::res::Response;

/// HTTPServer：无锁、并发、mut-less
pub struct HTTPServer {
    pub addr: SocketAddr,
    pub router: Arc<TrieNode>, // Trie 路由
}

impl HTTPServer {
    pub fn new(addr: SocketAddr, router: TrieNode) -> Self {
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
        router: Arc<TrieNode>,
        stream: TcpStream,
        peer_addr: SocketAddr
    ) -> std::io::Result<()> {
        let (reader, writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let mut writer = BufWriter::new(writer);

        // 构建 Request
        let req = Request::new(&mut reader, peer_addr, "").await;
        let res = Response::new(&mut writer);

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

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{ collections::HashMap, sync::Arc };
    use futures::FutureExt;
    use tokio::io::{ BufReader, BufWriter, AsyncReadExt, AsyncWriteExt };
    use tokio::net::{ TcpListener, TcpStream };
    use std::net::SocketAddr;

    use crate::{
        handler::HTTPContext,
        res::Response,
        req::Request,
        trie::{ TrieNode, NodeType, handle_request },
    };

    /// 简单帮助函数：生成 HTTPServer 并在 background 运行
    async fn spawn_server(root: TrieNode) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let router = Arc::new(root);

        tokio::spawn(async move {
            loop {
                let (stream, peer_addr) = listener.accept().await.unwrap();
                let router = router.clone();
                tokio::spawn(async move {
                    let (reader, writer) = stream.into_split();
                    let mut reader = BufReader::new(reader);
                    let mut writer = BufWriter::new(writer);

                    let req = Request::new(&mut reader, peer_addr, "").await;
                    let res = Response::new(&mut writer);

                    let mut ctx = HTTPContext {
                        req,
                        res,
                        global: Default::default(),
                        local: Default::default(),
                    };

                    handle_request(&router, &mut ctx).await;

                    let resp_str = ctx.res.body.join("\r\n");
                    Response::<'_>::write_str(&mut writer, &resp_str).await.unwrap();
                });
            }
        });

        addr
    }

    #[tokio::test]
    async fn test_trie_server_get() {
        // 构建 Trie
        let mut root = TrieNode::new(NodeType::Static("root".into()));
        root.insert(
            "/hello",
            Some("GET"),
            Arc::new(|ctx|
                (
                    async move {
                        ctx.res.body.push("world".to_string());
                        true
                    }
                ).boxed()
            ),
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
        let mut root = TrieNode::new(NodeType::Static("root".into()));
        root.insert(
            "/user/:id/profile",
            Some("POST"),
            Arc::new(|ctx|
                (
                    async move {
                        ctx.res.body.push("posted".to_string());
                        true
                    }
                ).boxed()
            ),
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
        let mut root = TrieNode::new(NodeType::Static("root".into()));

        root.insert(
            "/user/:id",
            Some("GET"),
            Arc::new(|ctx|
                (
                    async move {
                        println!("Inside id handler! ");

                        // 假设 ctx.req.params.data 是 HashMap<String, String>，这里不生成新 String

                        if let Some(params) = &ctx.req.params.data {
                            if let Some(id) = params.get("id") {
                                println!("get id {} ", id);
                                // 直接使用原始 &str
                                ctx.res.body.push(id.to_string()); // 生命周期和 params 一致
                            }
                        }
                        true
                    }
                ).boxed()
            ),
            None
        );

        let addr = spawn_server(root).await;

        let mut client = TcpStream::connect(addr).await.unwrap();
        client.write_all(b"GET /user/42 HTTP/1.1\r\nHost: x\r\n\r\n").await.unwrap();

        let mut resp = vec![0; 1024];
        let n = client.read(&mut resp).await.unwrap();
        let resp_str = std::str::from_utf8(&resp[..n]).unwrap();

        println!("resp_str {}", resp_str);

        assert!(resp_str.contains("42"));
    }
}
