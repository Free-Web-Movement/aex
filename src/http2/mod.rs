//! HTTP/2 Codec for TCP pipeline integration
//! 
//! This module provides HTTP/2 support as a codec that can be integrated
//! into the existing TCP pipeline architecture.

use std::sync::Arc;
use bytes::Bytes;
use http::Response;
use h2::server;
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;

use crate::connection::context::Context;
use crate::connection::global::GlobalContext;
use crate::http::meta::HttpMetadata;
use crate::http::middlewares::websocket::WebSocket;
use crate::http::protocol::content_type::ContentType;
use crate::http::protocol::header::HeaderKey;
use crate::http::protocol::method::HttpMethod;
use crate::http::protocol::status::StatusCode as AexStatusCode;
use crate::http::protocol::version::HttpVersion;
use crate::http::router::Router as HttpRouter;

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
                            let method_str = request.method().as_str();
                            
                            tracing::info!("[H2] {} {}", method_str, path);
                            
                            // Parse HTTP method
                            let http_method = HttpMethod::from_str(method_str).unwrap_or(HttpMethod::GET);
                            
                            // Build HttpMetadata from HTTP/2 request headers
                            let mut meta = HttpMetadata::new();
                            meta.method = http_method;
                            meta.path = path.clone();
                            meta.version = HttpVersion::Http20;
                            
                            // Copy headers from HTTP/2 request
                            for (name, value) in request.headers() {
                                if let Some(header_key) = HeaderKey::from_str(name.as_str()) {
                                    if let Ok(val) = value.to_str() {
                                        meta.headers.insert(header_key, val.to_string());
                                    }
                                }
                            }
                            
                            // Parse content type
                            if let Some(ct) = meta.headers.get(&HeaderKey::ContentType) {
                                meta.content_type = ContentType::parse(ct);
                            }
                            
                            // Check for WebSocket upgrade request (RFC8441 for HTTP/2)
                            let is_ws = WebSocket::check(http_method, &meta.headers);
                            
                            // Create Context - for HTTP/2 we need to handle stream specially
                            // Currently, HTTP/2 WebSocket support is detected but requires
                            // additional h2 stream handling for full support
                            let mut ctx = Context::new(None, None, self.global.clone(), peer_addr);
                            ctx.set(meta);
                            
                            // Execute router
                            let _route_found = self.router.on_request(&mut ctx).await;
                            
                            // Check if WebSocket upgrade was performed by middleware
                            let ws_upgraded = {
                                let meta = ctx.local.get_ref::<HttpMetadata>().unwrap();
                                meta.is_websocket
                            };
                            
                            // Note: Full HTTP/2 WebSocket support requires using h2's stream interface
                            // For now, we detect the upgrade request. Full implementation would need
                            // to use the h2 send_stream for WebSocket frames.
                            if is_ws || ws_upgraded {
                                tracing::info!("[H2] WebSocket upgrade requested (HTTP/2 WS support coming soon)");
                            }
                            
                            // Build response
                            let (status, body, headers) = {
                                let meta = ctx.local.get_ref::<HttpMetadata>().unwrap();
                                let status = meta.status.to_http_status();
                                let body = String::from_utf8_lossy(&meta.body).to_string();
                                let headers = meta.headers.clone();
                                (status, body, headers)
                            };
                            
                            // Build HTTP/2 response
                            let mut resp_builder = Response::builder().status(status);
                            
                            // Copy headers to HTTP/2 response
                            for (key, val) in headers.iter() {
                                if let Ok(h2_name) = http::header::HeaderName::from_bytes(key.as_str().as_bytes()) {
                                    resp_builder = resp_builder.header(h2_name, val.as_str());
                                }
                            }
                            
                            let resp: Response<()> = resp_builder
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