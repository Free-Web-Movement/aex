//! HTTP/1.1 protocol handler.
//!
//! Owns its detector ([`Http11Detector`]), its router/handler configuration,
//! and the [`WebSocket`](super::websocket) sub-handler: when a parsed request is
//! a WebSocket upgrade, the connection is handed off to the `websocket` child
//! instead of being served as a normal request.

use std::sync::Arc;

use bytes::Bytes;

use crate::connection::context::Context;
use crate::http::meta::HttpMetadata;
use crate::http::router::Router as HttpRouter;
use crate::protocols::classify::HTTP_METHODS;
use crate::protocols::websocket::{self, WebsocketConfig};
use crate::unified::detect::{DetectionState, ProtocolDetector, Verdict};
use crate::unified::handler::{Outcome, ProtocolHandler};

/// HTTP/1.1 request handler.
pub type HttpHandler =
    Arc<dyn Fn(&mut Context) -> futures::future::BoxFuture<'_, bool> + Send + Sync>;

/// Configuration for the HTTP/1.1 handler.
#[derive(Clone)]
pub struct Http1Config {
    /// The HTTP router serving requests when no explicit handler is set.
    pub router: Option<Arc<HttpRouter>>,
    /// Optional explicit request handler, taking precedence over the router.
    pub handler: Option<HttpHandler>,
    #[cfg(feature = "proxy")]
    /// Serve absolute-form requests and CONNECT tunnels as a forward proxy.
    pub http_proxy_enabled: bool,
    #[cfg(feature = "proxy")]
    /// Shared proxy credential check.
    pub proxy_authorizer: Option<crate::proxy::ProxyAuthorizer>,
}

impl Http1Config {
    /// A config with no router and no explicit handler.
    pub fn new() -> Self {
        Self {
            router: None,
            handler: None,
            #[cfg(feature = "proxy")]
            http_proxy_enabled: false,
            #[cfg(feature = "proxy")]
            proxy_authorizer: None,
        }
    }
}

impl Default for Http1Config {
    fn default() -> Self {
        Self::new()
    }
}

/// Build the HTTP/1.1 [`ProtocolHandler`], carrying its detector and the
/// WebSocket sub-handler.
pub fn protocol(cfg: Http1Config) -> Arc<ProtocolHandler<Bytes>> {
    let ws = websocket::protocol(WebsocketConfig {
        router: cfg.router.clone(),
    });
    let http = ProtocolHandler::new(
        "http",
        move |_handler: &ProtocolHandler<Bytes>, ctx: &mut Context| {
            let cfg = cfg.clone();
            Box::pin(async move { handle(&cfg, ctx).await })
        },
    )
    .with_detector(Arc::new(Http11Detector))
    .with_sub_handler(ws)
    .with_buffered_writer(true);
    Arc::new(http)
}

/// Serve one HTTP/1.1 connection.
pub async fn handle(cfg: &Http1Config, ctx: &mut Context) -> Outcome {
    if ctx.req().parse_to_local().await.is_err() {
        let _ = ctx.res().send_failure().await;
        return Outcome::Done;
    }

    #[cfg(feature = "proxy")]
    if cfg.http_proxy_enabled
        && crate::proxy::maybe_handle_http_proxy(ctx, cfg.proxy_authorizer.as_ref()).await
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

    let handled = if let Some(handler) = &cfg.handler {
        handler(ctx).await
    } else if let Some(router) = &cfg.router {
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
