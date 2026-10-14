//! WebSocket sub-protocol handler.
//!
//! WebSocket is not detected independently; it is reached by handing a parsed
//! HTTP/1.1 upgrade request off from the [`Http1`](crate::protocols::http1::Http1)
//! parent. The upgrade itself is performed by the websocket middleware
//! registered on the router, so this handler simply routes the request and
//! finalizes the response exactly as the previous inline `is_websocket` branch
//! did.

use std::sync::Arc;

use bytes::Bytes;

use crate::connection::context::Context;
use crate::handler::{CommandHandler, CommandKey, CommandRegistry, Outcome};
use crate::http::router::Router as HttpRouter;
use crate::http::types::Executor;
use crate::protocol::{Protocol, Transport};

/// WebSocket sub-protocol: routes upgrade requests through its router.
pub struct Websocket {
    router: Option<Arc<HttpRouter>>,
    commands: CommandRegistry<Bytes>,
    middleware: Vec<Arc<Executor>>,
}

impl Websocket {
    /// The WebSocket sub-handler, routing over `router` (commonly the same
    /// router as the HTTP/1.1 parent, which holds the upgrade middleware).
    pub fn new(router: Option<Arc<HttpRouter>>) -> Self {
        Self {
            router,
            commands: CommandRegistry::new(),
            middleware: Vec::new(),
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

    /// Serve one WebSocket upgrade request (struct-specific processing).
    pub async fn handle(&self, ctx: &mut Context) -> Outcome {
        let handled = if let Some(router) = &self.router {
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
}

impl Protocol for Websocket {
    fn name(&self) -> &str {
        "websocket"
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

    fn transport(&self) -> Transport {
        Transport::Stream
    }

    fn process<'a>(&'a self, ctx: &'a mut Context) -> futures::future::BoxFuture<'a, Outcome> {
        Box::pin(async move { self.handle(ctx).await })
    }
}
