#[cfg(test)]
mod tests {
    use super::*;
    use aex::tcp::listeners::{Listener, TCPHandler};
    use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::{TcpListener, TcpStream}};
    use std::{net::SocketAddr, sync::Arc};

    #[tokio::test]
    async fn test_tcp_handler_full_flow() {
        // 1. 初始化并绑定端口
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let mut handler_obj = TCPHandler {
            addr,
            listener: None,
        };
        handler_obj.listen().await.expect("Listen failed");
        
        let local_addr = handler_obj.listener.as_ref().unwrap().local_addr().unwrap();

        // 2. 解决生命周期问题：
        // 将 handler 包装在 Arc 中，这样我们可以 move 它的克隆进 async 块
        let handler_arc = Arc::new(handler_obj);
        let handler_clone = Arc::clone(&handler_arc);

        // 3. 运行 accept
        let accept_handle = tokio::spawn(async move {
            handler_clone.accept(|mut stream, _addr| async move {
                let _ = stream.write_all(b"hello").await;
            }).await.expect("Accept loop failed");
        });

        // 4. 模拟客户端
        let mut client = TcpStream::connect(local_addr).await.expect("Connect failed");
        let mut buffer = [0u8; 5];
        client.read_exact(&mut buffer).await.unwrap();
        assert_eq!(&buffer, b"hello");

        // 强行终止以覆盖 loop
        accept_handle.abort();
    }

    #[tokio::test]
    async fn test_accept_without_listen_error_path() {
        // 覆盖 ok_or_else 错误分支
        let handler_obj = TCPHandler {
            addr: "127.0.0.1:0".parse().unwrap(),
            listener: None,
        };

        let result = handler_obj.accept(|_, _| async {}).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Listener not bound"));
    }

    #[tokio::test]
    async fn test_listen_on_invalid_address() {
        // 覆盖 listen() 失败分支（例如使用特权端口或已被占用端口）
        // 尝试绑定到一个已存在的 Listener 端口
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let _guard = TcpListener::bind(addr).await.unwrap();
        let actual_addr = _guard.local_addr().unwrap();

        let mut handler_err = TCPHandler {
            addr: actual_addr,
            listener: None,
        };

        let result = handler_err.listen().await;
        assert!(result.is_err(), "Should fail when port is already taken");
    }
}