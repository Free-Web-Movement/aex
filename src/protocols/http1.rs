//! HTTP/1.1 protocol handler.
//!
//! Owns its detector ([`Http11Detector`]), its router/handler, and the
//! [`WebSocket`](super::websocket::Websocket) sub-handler: when a parsed request
//! is a WebSocket upgrade, the connection is handed off to the `websocket`
//! child instead of being served as a normal request.

use std::sync::Arc;

use bytes::Bytes;

use crate::connection::context::Context;
use crate::detect::{DetectionState, ProtocolDetector, Verdict};
use crate::handler::{CommandHandler, CommandKey, CommandRegistry, Outcome};
use crate::http::meta::HttpMetadata;
use crate::http::router::Router as HttpRouter;
use crate::http::types::Executor;
use crate::protocol::{Protocol, Transport};
use crate::protocols::classify::HTTP_METHODS;
use crate::protocols::websocket::Websocket;

/// HTTP/1.1 request handler.
pub type HttpHandler =
    Arc<dyn Fn(&mut Context) -> futures::future::BoxFuture<'_, bool> + Send + Sync>;

/// HTTP/1.1 protocol: routes requests and serves them via its router/handler.
pub struct Http1 {
    router: Option<Arc<HttpRouter>>,
    handler: Option<HttpHandler>,
    commands: CommandRegistry<Bytes>,
    middleware: Vec<Arc<Executor>>,
    sub_handlers: Vec<Arc<dyn Protocol>>,
    #[cfg(feature = "proxy")]
    http_proxy_enabled: bool,
    #[cfg(feature = "proxy")]
    proxy_authorizer: Option<crate::proxy::ProxyAuthorizer>,
}

impl Http1 {
    /// The HTTP/1.1 protocol. `router` serves requests unless an explicit
    /// `handler` is given (which then takes precedence).
    pub fn new(router: Option<Arc<HttpRouter>>, handler: Option<HttpHandler>) -> Self {
        let ws: Arc<dyn Protocol> = Arc::new(Websocket::new(router.clone()));
        Self {
            router,
            handler,
            commands: CommandRegistry::new(),
            middleware: Vec::new(),
            sub_handlers: vec![ws],
            #[cfg(feature = "proxy")]
            http_proxy_enabled: false,
            #[cfg(feature = "proxy")]
            proxy_authorizer: None,
        }
    }

    /// Append one protocol middleware (an [`Executor`]).
    pub fn with_middleware(mut self, middleware: Arc<Executor>) -> Self {
        self.middleware.push(middleware);
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

    /// Serve absolute-form requests and CONNECT tunnels as a forward proxy.
    /// `authorizer` is the shared credential check; `None` runs an open proxy.
    #[cfg(feature = "proxy")]
    pub fn with_proxy(mut self, authorizer: Option<crate::proxy::ProxyAuthorizer>) -> Self {
        self.http_proxy_enabled = true;
        self.proxy_authorizer = authorizer;
        self
    }

    /// Whether forward-proxy serving is enabled.
    #[cfg(feature = "proxy")]
    pub fn proxy_enabled(&self) -> bool {
        self.http_proxy_enabled
    }

    /// Serve one HTTP/1.1 connection (struct-specific processing).
    pub async fn handle(&self, ctx: &mut Context) -> Outcome {
        if ctx.req().parse_to_local().await.is_err() {
            let _ = ctx.res().send_failure().await;
            return Outcome::Done;
        }

        #[cfg(feature = "proxy")]
        if self.http_proxy_enabled
            && crate::proxy::maybe_handle_http_proxy(ctx, self.proxy_authorizer.as_ref()).await
        {
            return Outcome::Done; // connection fully served as proxy traffic
        }

        let is_ws = ctx
            .local
            .get_ref::<HttpMetadata>()
            .map(|m| m.is_websocket)
            .unwrap_or(false);

        if is_ws {
            return Outcome::Handoff("websocket".to_string());
        }

        let handled = if let Some(handler) = &self.handler {
            handler(ctx).await
        } else if let Some(router) = &self.router {
            router.on_request(ctx).await
        } else {
            false
        };

        if handled {
            let _ = ctx.res().send_response().await;
        } else {
            let _ = ctx.res().send_failure().await;
        }
        Outcome::Done
    }

    /// Replace the built-in WebSocket sub-handler (which defaults to a
    /// [`Websocket`] built from the same router). The WebSocket protocol is
    /// only ever reached through the HTTP upgrade, so it must live here rather
    /// than be mounted on its own.
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

    /// Box this protocol for registration.
    pub fn shared(self) -> Arc<Self> {
        Arc::new(self)
    }
}

impl Protocol for Http1 {
    fn name(&self) -> &str {
        "http"
    }

    fn detector(&self) -> Option<Arc<dyn ProtocolDetector>> {
        Some(Arc::new(Http11Detector))
    }

    fn router(&self) -> Option<Arc<HttpRouter>> {
        self.router.clone()
    }

    fn commands(&self) -> &CommandRegistry<Bytes> {
        &self.commands
    }

    fn middleware(&self) -> &[Arc<Executor>] {
        &self.middleware
    }

    fn sub_handlers(&self) -> &[Arc<dyn Protocol>] {
        &self.sub_handlers
    }

    fn transport(&self) -> Transport {
        Transport::Stream
    }

    fn buffered_writer(&self) -> bool {
        true
    }

    fn process<'a>(&'a self, ctx: &'a mut Context) -> futures::future::BoxFuture<'a, Outcome> {
        Box::pin(async move { self.handle(ctx).await })
    }
}

/// Detector for plaintext HTTP/1.x requests.
pub struct Http11Detector;

impl ProtocolDetector for Http11Detector {
    fn name(&self) -> &str {
        "http11"
    }

    fn protocol(&self) -> &str {
        "http"
    }

    fn max_need(&self) -> Option<usize> {
        Some(HTTP_METHODS.iter().map(|m| m.len()).max().unwrap_or(0))
    }

    fn detect(&self, buf: &[u8], _state: &mut DetectionState) -> Verdict {
        for m in HTTP_METHODS {
            if buf.starts_with(m) {
                return Verdict::Match;
            }
        }
        // A method token may straddle the buffer boundary; ask for just
        // enough bytes to rule the shortest pending method in or out.
        match pending_method_need(buf) {
            Some(n) => Verdict::NeedMore(n),
            None => Verdict::Pass,
        }
    }
}

/// Bytes needed before a still-possible method token can be decided, or
/// `None` when no known method has `buf` as a proper prefix anymore.
fn pending_method_need(buf: &[u8]) -> Option<usize> {
    HTTP_METHODS
        .iter()
        .filter(|m| m.len() > buf.len() && m.starts_with(buf))
        .map(|m| m.len() - buf.len())
        .min()
}
