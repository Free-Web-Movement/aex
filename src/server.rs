use std::io::{ self, Write };
use std::{ net::SocketAddr, sync::Arc };
use tokio::net::TcpListener;
use tokio::net::TcpStream;

use crate::router::Router;

/// HTTPServer：无锁、并发、mut-less
pub struct HTTPServer {
    pub addr: SocketAddr,
    pub router: Arc<Router>,
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
        io::stdout().flush().unwrap(); // 强制刷新

        loop {
            let (stream, peer_addr) = listener.accept().await?;
            let router = self.router.clone();

            // 每个连接独立任务
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
    ) -> std::io::Result<()> {
        // 捕获整个处理流程的错误
        router.on_request(stream, peer_addr).await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{ net::TcpStream, io::{ AsyncWriteExt, AsyncReadExt }, sync::oneshot };
    use std::net::SocketAddr;
    use crate::router::Router;

    /// Helper: 启动一个 HTTPServer 并可通过 oneshot 退出
    async fn spawn_test_server(addr: SocketAddr) -> oneshot::Sender<()> {
        let router = Router::new();
        let server = HTTPServer::new(addr, router);
        let (tx, rx) = oneshot::channel::<()>();

        tokio::spawn(async move {
            tokio::select! {
                _ = server.run() => {},
                _ = rx => {
                    println!("[TEST] Server exiting via signal");
                }
            }
        });

        tx
    }

    #[tokio::test]
    async fn test_server_start_and_shutdown() {
        let addr: SocketAddr = "127.0.0.1:7878".parse().unwrap();
        let shutdown_tx = spawn_test_server(addr).await;

        // 等待 server 启动
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // 测试 TCP 连接建立
        let mut stream = TcpStream::connect(addr).await.expect("Failed to connect");
        let request = b"GET /nonexistent HTTP/1.1\r\nHost: localhost\r\n\r\n";
        stream.write_all(request).await.unwrap();

        // 读取响应
        let mut buf = vec![0u8; 1024];
        let n = stream.read(&mut buf).await.unwrap();
        let resp = String::from_utf8_lossy(&buf[..n]);

        // 非法路由返回 404
        assert!(resp.contains("404"));

        // 通过 oneshot 信号退出 server
        let _ = shutdown_tx.send(());
    }

    #[tokio::test]
    async fn test_multiple_connections() {
        let addr: SocketAddr = "127.0.0.1:7879".parse().unwrap();
        let shutdown_tx = spawn_test_server(addr).await;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let mut handles = vec![];

        for _ in 0..5 {
            let addr = addr.clone();
            handles.push(
                tokio::spawn(async move {
                    let mut stream = TcpStream::connect(addr).await.unwrap();
                    let request = b"GET /another HTTP/1.1\r\nHost: localhost\r\n\r\n";
                    stream.write_all(request).await.unwrap();
                    let mut buf = vec![0u8; 1024];
                    let n = stream.read(&mut buf).await.unwrap();
                    let resp = String::from_utf8_lossy(&buf[..n]);
                    assert!(resp.contains("404")); // 因为 router 为空
                })
            );
        }

        for handle in handles {
            handle.await.unwrap();
        }

        let _ = shutdown_tx.send(());
    }
}
