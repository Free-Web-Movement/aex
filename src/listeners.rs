use std::net::SocketAddr;
use tokio::net::{TcpListener, TcpStream};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

// Trait with methods returning boxed futures to avoid async_trait dependency
pub trait Listener {
    fn listen<'a>(&'a mut self) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'a>>;

    // accept: takes a handler that returns a Future
    fn accept<'a, F, Fut>(&'a self, handler: F) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'a>>
    where
        F: Fn(TcpStream, SocketAddr) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static;
}

pub struct TCPHandler {
    pub addr: SocketAddr,
    // 使用 Option 因为在 listen 之前可能没有绑定
    pub listener: Option<TcpListener>,
}

impl Listener for TCPHandler {
    fn listen<'a>(&'a mut self) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'a>> {
        Box::pin(async move {
            let listener = TcpListener::bind(self.addr).await?;
            println!("TCP listener bound to {}", self.addr);
            self.listener = Some(listener);
            Ok(())
        })
    }

    fn accept<'a, F, Fut>(&'a self, handler: F) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'a>>
    where
        F: Fn(TcpStream, SocketAddr) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        Box::pin(async move {
            // 将 handler 放入 Arc，以便在多个线程/协程中共享
            let handler = Arc::new(handler);

            let listener = self.listener.as_ref()
                .ok_or_else(|| anyhow::anyhow!("Listener not bound. Call listen() first."))?;

            loop {
                let (socket, addr) = listener.accept().await?;
                let handler_clone = Arc::clone(&handler);

                // 派发任务
                tokio::spawn(async move {
                    handler_clone(socket, addr).await;
                });
            }
        })
    }
}