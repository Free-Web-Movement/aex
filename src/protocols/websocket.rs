//! WebSocket sub-protocol handler.
//!
//! WebSocket is not detected independently; it is reached by handing a parsed
//! HTTP/1.1 upgrade request off from the [`http1`](super::http1) parent. The
//! upgrade itself is performed by the websocket middleware registered on the
//! router, so this handler simply routes the request and finalizes the response
//! exactly as the previous inline `is_websocket` branch did.

use std::sync::Arc;

use bytes::Bytes;

use crate::connection::context::Context;
use crate::http::router::Router as HttpRouter;
use crate::unified::handler::{Outcome, ProtocolHandler};

/// Configuration for the WebSocket sub-handler.
#[derive(Clone, Default)]
pub struct WebsocketConfig {
    /// The router holding the `WebSocket` middleware.
    pub router: Option<Arc<HttpRouter>>,
}

/// Build the WebSocket [`ProtocolHandler`] (a child of HTTP/1.1).
pub fn protocol(cfg: WebsocketConfig) -> Arc<ProtocolHandler<Bytes>> {
    Arc::new(ProtocolHandler::new(
        "websocket",
        move |_handler: &ProtocolHandler<Bytes>, ctx: &mut Context| {
            let cfg = cfg.clone();
            Box::pin(async move { handle(&cfg, ctx).await })
        },
    ))
}

/// Serve one WebSocket upgrade request.
pub async fn handle(cfg: &WebsocketConfig, ctx: &mut Context) -> Outcome {
    let handled = if let Some(router) = &cfg.router {
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
