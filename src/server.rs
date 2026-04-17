//! # Server
//!
//! HTTP Server implementation

use crate::connection::context::TypeMapExt;
use crate::connection::global::GlobalContext;
use crate::http::router::Router as HttpRouter;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;

/// HTTP Server
#[derive(Clone)]
pub struct Server {
    pub addr: SocketAddr,
    pub globals: Arc<GlobalContext>,
}

impl Server {
    pub fn new(addr: SocketAddr, globals: Option<Arc<GlobalContext>>) -> Self {
        Self {
            addr,
            globals: globals.unwrap_or_else(|| {
                Arc::new(GlobalContext::new(addr, None))
            }),
        }
    }

    pub fn http(mut self, router: HttpRouter) -> Self {
        self.globals.routers.set_value(Arc::new(router));
        self
    }

    pub async fn start(&self) -> anyhow::Result<()> {
        let router = self.globals.routers.get_value::<Arc<HttpRouter>>()
            .ok_or_else(|| anyhow::anyhow!("No HTTP router set"))?;
        let router = router.clone();
        let globals = self.globals.clone();
        let addr = self.addr;
        
        let listener = TcpListener::bind(addr).await?;
        tracing::info!("HTTP server listening on {}", addr);

        loop {
            match listener.accept().await {
                Ok((socket, peer_addr)) => {
                    let router = router.clone();
                    let globals = globals.clone();
                    tokio::spawn(async move {
                        use tokio::io::{BufReader, BufWriter};
                        
                        let (reader, writer) = socket.into_split();
                        let reader = Box::new(BufReader::new(reader)) as Box<dyn tokio::io::AsyncBufRead + Send + Sync + Unpin>;
                        let writer = Box::new(BufWriter::new(writer)) as Box<dyn tokio::io::AsyncWrite + Send + Sync + Unpin>;
                        
                        let mut ctx = crate::connection::context::Context::new(
                            Some(reader), Some(writer), globals, peer_addr,
                        );
                        
                        if ctx.req().parse_to_local().await.is_ok() {
                            if router.on_request(&mut ctx).await {
                                let _ = ctx.res().send_response().await;
                            } else {
                                let _ = ctx.res().send_failure().await;
                            }
                        }
                    });
                }
                Err(e) => {
                    tracing::warn!("Accept error: {}", e);
                }
            }
        }
    }
}

pub type HTTPServer = Server;
