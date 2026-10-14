//! HTTP/2 protocol handler.
//!
//! Owns its detector ([`Http2Detector`]) and its request handler. The
//! connection's split halves are re-united into a single `AsyncRead +
//! AsyncWrite` stream so the `h2` handshake can run, then each request stream
//! builds its own short-lived [`Context`] carrying an [`HttpMetadata`].

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll};

use bytes::Bytes;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use crate::connection::context::{BoxReader, BoxWriter, Context};
use crate::detect::{DetectionState, ProtocolDetector, Verdict};
use crate::handler::{CommandHandler, CommandKey, CommandRegistry, Outcome};
use crate::http::meta::HttpMetadata;
use crate::http::middlewares::websocket::WebSocket;
use crate::http::protocol::header::HeaderKey;
use crate::http::protocol::method::HttpMethod;
use crate::http::protocol::version::HttpVersion;
use crate::http::router::Router as HttpRouter;
use crate::protocol::{Protocol, Transport};
use crate::protocols::classify::H2_CONNECTION_PREFACE;

/// HTTP/2 request handler.
pub type Http2Handler =
    Arc<dyn Fn(&mut Context) -> futures::future::BoxFuture<'static, bool> + Send + Sync>;

/// HTTP/2 protocol: claims connections via the connection preface and serves
/// each request stream through its handler.
pub struct Http2 {
    router: Option<Arc<HttpRouter>>,
    handler: Option<Http2Handler>,
    commands: CommandRegistry<Bytes>,
    sub_handlers: Vec<Arc<dyn Protocol>>,
}

impl Http2 {
    /// The HTTP/2 protocol. `router` serves each request stream (same router
    /// mechanism as [`Http1`]); an explicit `handler` takes precedence. This is
    /// the same `(router, handler)` shape as `Http1::new`.
    ///
    /// [`Http1`]: crate::protocols::http1::Http1
    pub fn new(router: Option<Arc<HttpRouter>>, handler: Option<Http2Handler>) -> Self {
        Self {
            router,
            handler,
            commands: CommandRegistry::new(),
            sub_handlers: Vec::new(),
        }
    }

    /// Attach the router serving HTTP/2 requests.
    pub fn with_router(mut self, router: Arc<HttpRouter>) -> Self {
        self.router = Some(router);
        self
    }

    /// Register a protocol command handler.
    pub fn with_command<K: Into<CommandKey>>(
        mut self,
        key: K,
        handler: CommandHandler<Bytes>,
    ) -> Self {
        self.commands.register(key, handler);
        self
    }

    /// Attach a WebSocket sub-handler, reached when a request is a WebSocket
    /// upgrade (RFC 8441). Mirrors [`Http1::with_websocket`].
    ///
    /// [`Http1::with_websocket`]: crate::protocols::http1::Http1::with_websocket
    pub fn with_websocket(mut self, websocket: Arc<dyn Protocol>) -> Self {
        match self
            .sub_handlers
            .iter_mut()
            .find(|s| s.name() == "websocket")
        {
            Some(slot) => *slot = websocket,
            None => self.sub_handlers.push(websocket),
        }
        self
    }

    /// The attached WebSocket sub-handler, if any.
    pub fn websocket(&self) -> Option<Arc<dyn Protocol>> {
        self.sub_handlers
            .iter()
            .find(|s| s.name() == "websocket")
            .cloned()
    }

    /// Serve one HTTP/2 connection (struct-specific processing).
    pub async fn handle(&self, ctx: &mut Context) -> Outcome {
        let io = Reunited {
            reader: ctx.reader.take(),
            writer: ctx.writer.take(),
        };

        let mut conn = match h2::server::handshake(io).await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("[H2] handshake failed: {}", e);
                return Outcome::Done;
            }
        };

        let globals = ctx.global.clone();
        let peer_addr = ctx.addr;

        loop {
            tokio::select! {
                frame = conn.accept() => {
                    match frame {
                        Some(Ok((request, mut responder))) => {
                            let path = request.uri().path().to_string();
                            let method_str = request.method().as_str();
                            let http_method = HttpMethod::from_str(method_str).unwrap_or(HttpMethod::GET);

                            let mut meta = HttpMetadata::default();
                            meta.method = http_method;
                            meta.path = path.clone();
                            meta.version = HttpVersion::Http20;

                            for (name, value) in request.headers() {
                                if let Some(header_key) = HeaderKey::from_str(name.as_str()) {
                                    if let Ok(val) = value.to_str() {
                                        meta.headers.insert(header_key, val.to_string());
                                    }
                                }
                            }

                            let is_ws = WebSocket::check(http_method, &meta.headers);
                            if is_ws {
                                meta.is_websocket = true;
                            }

                            let mut stream_ctx = Context::new(None, None, globals.clone(), peer_addr);
                            stream_ctx.set(meta);

                            // A WebSocket upgrade request (RFC 8441) is handed to
                            // the attached `websocket` sub-handler, if any.
                            // Otherwise: explicit handler first, then the router
                            // (same precedence and router mechanism as Http1).
                            let ws_handler = if is_ws { self.websocket() } else { None };
                            if let Some(ws) = ws_handler {
                                let _ = ws.process(&mut stream_ctx).await;
                            } else if let Some(h) = &self.handler {
                                h(&mut stream_ctx).await;
                            } else if let Some(router) = &self.router {
                                router.on_request(&mut stream_ctx).await;
                            } else {
                                tracing::warn!("[H2] No HTTP/2 router or handler registered");
                            }

                            let meta = stream_ctx.local.get_ref::<HttpMetadata>();
                            let status = if let Some(m) = meta {
                                m.status.to_http_status()
                            } else {
                                http::StatusCode::OK
                            };
                            let mut body_str = String::new();
                            if let Some(m) = meta {
                                body_str = String::from_utf8_lossy(&m.body).to_string();
                            }

                            let resp_builder = http::Response::builder().status(status);

                            match resp_builder.body(()) {
                                Ok(resp) => {
                                    if let Ok(mut send_stream) = responder.send_response(resp, false) {
                                        let _ = send_stream.send_data(Bytes::from(body_str), true);
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!("[H2] build response failed: {}", e);
                                }
                            }
                        }
                        Some(Err(e)) => {
                            tracing::warn!("[H2] frame error: {}", e);
                        }
                        None => break,
                    }
                }
            }
        }
        Outcome::Done
    }

    /// Box this protocol for registration.
    pub fn shared(self) -> Arc<Self> {
        Arc::new(self)
    }
}

impl Protocol for Http2 {
    fn name(&self) -> &str {
        "http2"
    }

    fn detector(&self) -> Option<Arc<dyn ProtocolDetector>> {
        Some(Arc::new(Http2Detector))
    }

    fn router(&self) -> Option<Arc<HttpRouter>> {
        self.router.clone()
    }

    fn commands(&self) -> &CommandRegistry<Bytes> {
        &self.commands
    }

    fn sub_handlers(&self) -> &[Arc<dyn Protocol>] {
        &self.sub_handlers
    }

    fn transport(&self) -> Transport {
        Transport::Stream
    }

    fn process<'a>(&'a self, ctx: &'a mut Context) -> futures::future::BoxFuture<'a, Outcome> {
        Box::pin(async move { self.handle(ctx).await })
    }
}

/// Detector for the HTTP/2 connection preface.
pub struct Http2Detector;

impl ProtocolDetector for Http2Detector {
    fn name(&self) -> &str {
        "http2"
    }

    fn protocol(&self) -> &str {
        "http2"
    }

    fn max_need(&self) -> Option<usize> {
        Some(H2_CONNECTION_PREFACE.len())
    }

    fn detect(&self, buf: &[u8], _state: &mut DetectionState) -> Verdict {
        let preface = H2_CONNECTION_PREFACE;
        if buf.starts_with(preface) {
            return Verdict::Match;
        }
        let shared = preface
            .iter()
            .zip(buf.iter())
            .take_while(|(a, b)| a == b)
            .count();
        if shared > 0 {
            return Verdict::NeedMore(preface.len() - shared);
        }
        Verdict::Pass
    }
}

/// Re-unites the already-split connection halves into a single stream so a
/// protocol needing `AsyncRead + AsyncWrite` (the `h2` handshake) can run.
struct Reunited {
    reader: Option<BoxReader>,
    writer: Option<BoxWriter>,
}

impl AsyncRead for Reunited {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let r = self.reader.as_mut().expect("reader taken");
        Pin::new(r).poll_read(cx, buf)
    }
}

impl AsyncWrite for Reunited {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let w = self.writer.as_mut().expect("writer taken");
        Pin::new(w).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        let w = self.writer.as_mut().expect("writer taken");
        Pin::new(w).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
    ) -> Poll<std::io::Result<()>> {
        let w = self.writer.as_mut().expect("writer taken");
        Pin::new(w).poll_shutdown(cx)
    }
}
