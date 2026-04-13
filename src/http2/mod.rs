//! HTTP/2 Codec for TCP pipeline integration
//! 
//! This module provides HTTP/2 support as a codec that can be integrated
//! into the existing TCP pipeline architecture.

use std::sync::{Arc, Mutex};
use bytes::Bytes;
use http::{Response, StatusCode, header};
use h2::server;
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;

use crate::http::router::Router as HttpRouter;
use crate::connection::global::GlobalContext;

/// HTTP/2 response body storage (set by handlers)
pub static H2_RESPONSE_BODY: Mutex<String> = Mutex::new(String::new());

/// HTTP/2 minimal context for handler compatibility
#[derive(Default)]
pub struct H2Context {
    pub path: String,
    pub body: String,
}

impl H2Context {
    pub fn send(&mut self, content: &str, _mime: Option<crate::http::protocol::media_type::SubMediaType>) {
        *H2_RESPONSE_BODY.lock().unwrap() = content.to_string();
    }
}

/// HTTP/2 Codec that handles HTTP/2 connections
/// and integrates with the existing HTTP router
pub struct H2Codec {
    router: Arc<HttpRouter>,
    global: Arc<GlobalContext>,
}

impl H2Codec {
    pub fn new(router: Arc<HttpRouter>, global: Arc<GlobalContext>) -> Self {
        Self { router, global }
    }

    /// Handle an HTTP/2 connection
    pub async fn handle(
        &self,
        socket: TcpStream,
        peer_addr: std::net::SocketAddr,
        cancel_token: CancellationToken,
    ) -> anyhow::Result<()> {
        tracing::info!("[H2] Connection from {}", peer_addr);
        
        let mut conn = server::handshake(socket)
            .await
            .map_err(|e| anyhow::anyhow!("h2 handshake failed: {}", e))?;

        loop {
            tokio::select! {
                _ = cancel_token.cancelled() => {
                    break;
                }
                frame = conn.accept() => {
                    match frame {
                        Some(Ok((request, mut responder))) => {
                            let path = request.uri().path().to_string();
                            let method = request.method().as_str();
                            
                            tracing::info!("[H2] {} {}", method, path);
                            
                            // Clear previous response
                            *H2_RESPONSE_BODY.lock().unwrap() = String::new();
                            
                            // 使用 router 检查路由是否存在
                            let route_found = self.router.has_route(method, &path);
                            
                            let body = if route_found {
                                format!("[H2] Route found: {} {}", method, path)
                            } else {
                                "404 Not Found".to_string()
                            };
                            
                            let status = if route_found { StatusCode::OK } else { StatusCode::NOT_FOUND };
                            
                            let resp: Response<()> = Response::builder()
                                .status(status)
                                .header(header::CONTENT_TYPE, "text/plain")
                                .body(())
                                .map_err(|e| anyhow::anyhow!("build response failed: {}", e))?;

                            let mut send_stream = responder.send_response(resp, false)
                                .map_err(|e| anyhow::anyhow!("send_response failed: {}", e))?;
                            
                            send_stream.send_data(Bytes::from(body), true)
                                .map_err(|e| anyhow::anyhow!("send_data failed: {}", e))?;
                        }
                        Some(Err(e)) => {
                            tracing::warn!("h2 frame error: {}", e);
                        }
                        None => break,
                    }
                }
            }
        }

        Ok(())
    }

    /// Check if connection is HTTP/2 by inspecting connection preface
    pub async fn is_h2_connection(socket: &mut TcpStream) -> bool {
        use tokio::io::AsyncReadExt;
        
        let mut buf = [0u8; 24];
        match socket.read(&mut buf).await {
            Ok(n) if n >= 24 => {
                buf.starts_with(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n")
            }
            _ => false,
        }
    }
}