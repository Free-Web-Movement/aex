//! Raw TCP fallback protocol handler.
//!
//! Used for connections no detector claims, and as the wrapper for
//! user-registered `custom_handler`s. The user-facing closure takes ownership of
//! the [`Context`], so the handler reconstructs an owned context from the one
//! the runtime hands it.
//!
//! The [`Tcp`] struct is the concrete protocol (the catch-all default); custom
//! named protocols are built with [`from_handler`].

use std::sync::Arc;

use bytes::Bytes;

use crate::connection::context::Context;
use crate::http::types::Executor;
use crate::tcp::router::Router as TcpRouter;
use crate::tcp::types::{TCPCommand, TCPFrame};
use crate::unified::handler::{
    CommandHandler, CommandKey, CommandRegistry, Outcome, ProtocolHandler, take_context,
};
use crate::unified::{Protocol, Transport};

/// Raw TCP connection handler (takes ownership of the [`Context`]).
pub type TCPHandler = Arc<dyn Fn(Context) -> tokio::task::JoinHandle<()> + Send + Sync>;

/// Top-level TCP protocol handler.
///
/// TCP is the **only** stream transport at the top level; every other stream
/// protocol (HTTP, HTTP/2, WebSocket, SOCKS, NAT, …) is a **sub-handler** of it,
/// mirroring the real protocol stack. TCP accepts every stream connection and
/// hands it down to whichever child claims the detected protocol; connections no
/// child claims are served by the raw `handler` (or dropped when it is `None`).
pub struct Tcp {
    handler: Option<TCPHandler>,
    middleware: Vec<Arc<Executor>>,
    sub_handlers: Vec<Arc<dyn Protocol>>,
    commands: CommandRegistry<Bytes>,
}

impl Tcp {
    /// The top-level TCP protocol. `handler` serves raw connections no child
    /// claims; `None` drops them. Attach children with
    /// [`sub`](Self::sub).
    pub fn new(handler: Option<TCPHandler>) -> Self {
        Self {
            handler,
            middleware: Vec::new(),
            sub_handlers: Vec::new(),
            commands: CommandRegistry::new(),
        }
    }

    /// Append one protocol middleware (an [`Executor`]).
    pub fn with_middleware(mut self, middleware: Arc<Executor>) -> Self {
        self.middleware.push(middleware);
        self
    }

    /// Append a recursive sub-handler (HTTP, HTTP/2, SOCKS, NAT, …).
    pub fn sub(mut self, sub: Arc<dyn Protocol>) -> Self {
        self.sub_handlers.push(sub);
        self
    }

    /// Register a protocol command handler.
    pub fn with_command<K: Into<CommandKey>>(mut self, key: K, handler: CommandHandler<Bytes>) -> Self {
        self.commands.register(key, handler);
        self
    }

    /// Serve one raw TCP connection (struct-specific processing).
    pub async fn handle(&self, ctx: &mut Context) -> Outcome {
        match &self.handler {
            Some(h) => {
                let owned = take_context(ctx);
                h(owned);
            }
            None => {
                tracing::warn!(
                    "[Unified] No TCP handler registered, dropping connection from {}",
                    ctx.addr
                );
            }
        }
        Outcome::Done
    }

    /// Box this protocol for registration.
    pub fn shared(self) -> Arc<Self> {
        Arc::new(self)
    }
}

impl Protocol for Tcp {
    fn name(&self) -> &str {
        "tcp"
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

    fn is_default(&self) -> bool {
        true
    }

    fn process<'a>(&'a self, ctx: &'a mut Context) -> futures::future::BoxFuture<'a, Outcome> {
        Box::pin(async move { self.handle(ctx).await })
    }
}

/// Adapt a [`TcpRouter`] into a [`TCPHandler`] suitable for [`Tcp::new`].
///
/// The router itself is unchanged; the returned closure hands it the shared
/// connection context on a background task, exactly as the previous
/// server-embedded TCP pipeline did.
pub fn router_handler<F, C>(router: Arc<TcpRouter<F, C>>) -> TCPHandler
where
    F: TCPFrame + Send + Sync + 'static,
    C: TCPCommand + Send + Sync + 'static,
{
    Arc::new(move |ctx: Context| {
        let router = router.clone();
        tokio::spawn(async move {
            let ctx = Arc::new(tokio::sync::Mutex::new(ctx));
            let _ = router.handle(ctx).await;
        })
    })
}

/// Build a [`ProtocolHandler`] named `name` wrapping a user [`TCPHandler`].
///
/// Custom protocols claimed by a detector are dispatched through this, so they
/// get the same state record, middleware and recursive machinery as built-ins.
pub fn from_handler(
    name: impl Into<String>,
    handler: Option<TCPHandler>,
) -> Arc<ProtocolHandler<Bytes>> {
    let name = name.into();
    let label = name.clone();
    Arc::new(
        ProtocolHandler::new(
            name,
            move |_handler: &ProtocolHandler<Bytes>, ctx: &mut Context| {
                let handler = handler.clone();
                let label = label.clone();
                Box::pin(async move {
                    match handler {
                        Some(h) => {
                            let owned = take_context(ctx);
                            h(owned);
                        }
                        None => {
                            tracing::warn!(
                                "[Unified] No {} handler registered, dropping connection",
                                label
                            );
                        }
                    }
                    Outcome::Done
                })
            },
        )
    )
}