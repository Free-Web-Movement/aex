//! UDP protocol handler.
//!
//! Every datagram builds a short-lived [`Context`] carrying the raw payload; the
//! handler reconstructs an owned context for the user-facing closure.

use std::sync::Arc;

use bytes::Bytes;

use crate::connection::context::Context;
use crate::http::types::Executor;
use crate::unified::handler::{CommandHandler, CommandKey, CommandRegistry, Outcome, take_context};
use crate::unified::{Protocol, Transport};

/// UDP datagram handler (takes ownership of the [`Context`]).
pub type UDPHandler = Arc<dyn Fn(Context) -> tokio::task::JoinHandle<()> + Send + Sync>;

/// Top-level UDP protocol handler.
///
/// UDP is the **only** datagram transport at the top level; every datagram is
/// handed to the raw `handler`. Like [`Tcp`](crate::protocols::tcp::Tcp) it may
/// carry middleware and child handlers, mirroring the top-level TCP/UDP split.
pub struct Udp {
    handler: Option<UDPHandler>,
    middleware: Vec<Arc<Executor>>,
    sub_handlers: Vec<Arc<dyn Protocol>>,
    commands: CommandRegistry<Bytes>,
}

impl Udp {
    /// The top-level datagram protocol. `handler` is invoked for every datagram;
    /// `None` drops them.
    pub fn new(handler: Option<UDPHandler>) -> Self {
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

    /// Append a recursive sub-handler.
    pub fn sub(mut self, sub: Arc<dyn Protocol>) -> Self {
        self.sub_handlers.push(sub);
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

    /// Serve one UDP datagram (struct-specific processing).
    pub async fn handle(&self, ctx: &mut Context) -> Outcome {
        match &self.handler {
            Some(h) => {
                let owned = take_context(ctx);
                h(owned);
            }
            None => {
                tracing::warn!("[Unified] No UDP handler registered, dropping datagram");
            }
        }
        Outcome::Done
    }

    /// Box this protocol for registration.
    pub fn shared(self) -> Arc<Self> {
        Arc::new(self)
    }
}

impl Protocol for Udp {
    fn name(&self) -> &str {
        "udp"
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
        Transport::Datagram
    }

    fn is_default(&self) -> bool {
        true
    }

    fn process<'a>(&'a self, ctx: &'a mut Context) -> futures::future::BoxFuture<'a, Outcome> {
        Box::pin(async move { self.handle(ctx).await })
    }
}