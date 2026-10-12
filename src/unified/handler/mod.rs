//! # Unified Protocol Handler
//!
//! aex's core is a *protocol handler*. A handler bundles everything a protocol
//! needs to plug into the unified server:
//!
//! - a **detector** — how connections are claimed for this protocol;
//! - a per-connection **state record** ([`HandlerState`]), shared through the
//!   [`Context`]'s local type map so the whole downstream chain can query it;
//! - **middleware** — the exact same [`Executor`] chain the HTTP router uses
//!   (`Fn(&mut Context) -> BoxFuture<bool>`); a middleware returning `false`
//!   short-circuits the handler. No exotic combinators, no `Next`;
//! - **recursive sub-handlers** — a handler may hand the connection off to a
//!   named child (HTTP → WebSocket, TLS → HTTP, ...), which is itself a full
//!   protocol handler and runs its own detector/middleware/children;
//! - **protocol command handlers** ([`CommandRegistry`]) — dispatch decoded
//!   commands by name or numeric id.
//!
//! Command payloads are generic over `C`, defaulting to [`Bytes`]. The unified
//! server uses the universal byte instantiation so business layers can decode
//! commands without aex knowing the concrete message type.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use bytes::Bytes;
use futures::future::BoxFuture;

use crate::connection::context::Context;
use crate::http::types::Executor;
use crate::unified::detect::{Claim, ProtocolDetector};
use crate::unified::{Protocol, Transport};

/// Move every field out of `ctx`, leaving it empty.
///
/// Terminal handlers whose user-facing closure takes ownership of the
/// [`Context`] (the raw TCP and UDP handlers) reconstruct that owned context
/// through this helper, preserving the reader/writer halves and the local type
/// map (fd, detection state, handler state).
pub fn take_context(ctx: &mut Context) -> Context {
    Context {
        addr: ctx.addr,
        local_addr: ctx.local_addr,
        accepted: ctx.accepted,
        reader: ctx.reader.take(),
        writer: ctx.writer.take(),
        global: ctx.global.clone(),
        local: std::mem::take(&mut ctx.local),
    }
}

/// Result of running a protocol handler over one connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The handler processed the connection to completion.
    Done,
    /// The handler did not accept the connection; the caller may fall back.
    Unhandled,
    /// Hand the connection off to the named sub-handler.
    Handoff(String),
}

impl Outcome {
    /// Whether the handler finished the connection (either by handling it or by
    /// explicitly declining it).
    pub fn is_finished(&self) -> bool {
        matches!(self, Outcome::Done | Outcome::Unhandled)
    }
}

/// Per-connection protocol state record.
///
/// Installed into [`Context::local`] before the root handler runs and updated
/// as the connection descends through sub-handlers, so middleware, commands and
/// user code can observe which protocol is currently serving the connection.
pub struct HandlerState {
    /// Active handler chain, outermost first (`["http", "websocket"]`).
    pub path: Vec<String>,
    /// When the first handler accepted the connection.
    pub started_at: Instant,
    /// Bytes read from the peer.
    pub bytes_in: u64,
    /// Bytes written to the peer.
    pub bytes_out: u64,
    /// Number of dispatched commands, keyed by command label.
    pub commands: HashMap<String, u64>,
    /// Set once the connection is finished.
    pub closed: bool,
    /// Handler-private scratch data, keyed by type.
    scratch: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl Default for HandlerState {
    fn default() -> Self {
        Self {
            path: Vec::new(),
            started_at: Instant::now(),
            bytes_in: 0,
            bytes_out: 0,
            commands: HashMap::new(),
            closed: false,
            scratch: HashMap::new(),
        }
    }
}

impl HandlerState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that `name` has taken over the connection.
    pub fn enter(&mut self, name: &str) {
        self.path.push(name.to_string());
    }

    /// Name of the handler currently serving the connection.
    pub fn current(&self) -> Option<&str> {
        self.path.last().map(|s| s.as_str())
    }

    /// Count one dispatched command.
    pub fn record_command(&mut self, name: &str) {
        *self.commands.entry(name.to_string()).or_insert(0) += 1;
    }

    /// Mark the connection finished.
    pub fn close(&mut self) {
        self.closed = true;
    }

    /// Store handler-private scratch data keyed by type.
    pub fn set_scratch<T: Send + Sync + 'static>(&mut self, val: T) {
        self.scratch.insert(TypeId::of::<T>(), Box::new(val));
    }

    /// Fetch a clone of previously stored scratch data.
    pub fn get_scratch<T: Clone + 'static>(&self) -> Option<T> {
        self.scratch
            .get(&TypeId::of::<T>())
            .and_then(|b| b.downcast_ref::<T>().cloned())
    }

    /// Mutably access previously stored scratch data.
    pub fn get_scratch_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.scratch
            .get_mut(&TypeId::of::<T>())
            .and_then(|b| b.downcast_mut::<T>())
    }
}

/// Key identifying a command handler, by label or numeric id.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CommandKey {
    Name(String),
    Id(u32),
}

impl CommandKey {
    /// Human-readable label used when recording the command in [`HandlerState`].
    pub fn label(&self) -> String {
        match self {
            CommandKey::Name(n) => n.clone(),
            CommandKey::Id(id) => id.to_string(),
        }
    }
}

impl From<&str> for CommandKey {
    fn from(s: &str) -> Self {
        CommandKey::Name(s.to_string())
    }
}

impl From<String> for CommandKey {
    fn from(s: String) -> Self {
        CommandKey::Name(s)
    }
}

impl From<u32> for CommandKey {
    fn from(id: u32) -> Self {
        CommandKey::Id(id)
    }
}

/// A protocol command handler: receives the owning [`Context`] and the decoded
/// command payload `C`.
pub type CommandHandler<C> =
    Arc<dyn for<'a> Fn(&'a mut Context, C) -> BoxFuture<'a, Outcome> + Send + Sync>;

/// Registry of protocol command handlers, keyed by name or numeric id.
pub struct CommandRegistry<C = Bytes> {
    handlers: HashMap<CommandKey, CommandHandler<C>>,
}

impl<C> Default for CommandRegistry<C> {
    fn default() -> Self {
        Self::new()
    }
}

impl<C> CommandRegistry<C> {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// Register a command handler under `key`.
    pub fn register<K: Into<CommandKey>>(&mut self, key: K, handler: CommandHandler<C>) {
        self.handlers.insert(key.into(), handler);
    }

    /// Whether a handler is registered for `key`.
    pub fn contains(&self, key: &CommandKey) -> bool {
        self.handlers.contains_key(key)
    }

    /// Number of registered command handlers.
    pub fn len(&self) -> usize {
        self.handlers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }

    /// Dispatch one command, recording it in the connection's [`HandlerState`].
    /// Returns [`Outcome::Unhandled`] when no handler is registered.
    pub async fn dispatch(&self, ctx: &mut Context, key: CommandKey, payload: C) -> Outcome {
        match self.handlers.get(&key) {
            Some(handler) => {
                if let Some(state) = ctx.local.get_mut::<HandlerState>() {
                    state.record_command(&key.label());
                }
                handler(ctx, payload).await
            }
            None => Outcome::Unhandled,
        }
    }
}

/// The processing function of a protocol handler.
///
/// Implemented for any closure `Fn(&ProtocolHandler<C>, &mut Context)` that
/// returns a boxed future, so handlers can be written inline while still being
/// able to reach their own command registry via the passed handler.
pub trait HandlerFn<C = Bytes>: Send + Sync {
    fn call<'a>(
        &'a self,
        handler: &'a ProtocolHandler<C>,
        ctx: &'a mut Context,
    ) -> BoxFuture<'a, Outcome>;
}

impl<C, F> HandlerFn<C> for F
where
    C: Send + Sync + 'static,
    F: for<'a> Fn(&'a ProtocolHandler<C>, &'a mut Context) -> BoxFuture<'a, Outcome> + Send + Sync,
{
    fn call<'a>(
        &'a self,
        handler: &'a ProtocolHandler<C>,
        ctx: &'a mut Context,
    ) -> BoxFuture<'a, Outcome> {
        self(handler, ctx)
    }
}

/// A recursive protocol handler node.
pub struct ProtocolHandler<C = Bytes> {
    name: String,
    detector: Option<Arc<dyn ProtocolDetector>>,
    middleware: Vec<Arc<Executor>>,
    sub_handlers: Vec<Arc<ProtocolHandler<C>>>,
    commands: CommandRegistry<C>,
    handler: Arc<dyn HandlerFn<C>>,
    buf_writer: bool,
    default: bool,
    datagram: bool,
}

impl<C> ProtocolHandler<C>
where
    C: Send + Sync + 'static,
{
    /// Create a protocol handler named `name` with processing closure `f`.
    pub fn new<F>(name: impl Into<String>, f: F) -> Self
    where
        F: for<'a> Fn(&'a ProtocolHandler<C>, &'a mut Context) -> BoxFuture<'a, Outcome>
            + Send
            + Sync
            + 'static,
    {
        Self {
            name: name.into(),
            detector: None,
            middleware: Vec::new(),
            sub_handlers: Vec::new(),
            commands: CommandRegistry::new(),
            handler: Arc::new(f),
            buf_writer: false,
            default: false,
            datagram: false,
        }
    }

    /// Unique name of this handler (also its protocol label).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether this handler owns the connection described by `claim`.
    ///
    /// This is the *only* place a protocol decides ownership — the server's
    /// dispatch is a plain loop over handlers. Ownership is decided by:
    /// a [`default`](Self::with_default) handler accepts everything; otherwise a
    /// handler whose own detector is the one that matched, or whose name equals
    /// the claimed protocol label.
    pub fn is_mine(&self, claim: Option<&Claim>) -> bool {
        if self.default {
            return true;
        }
        match claim {
            Some(claim) => {
                self.detector
                    .as_ref()
                    .is_some_and(|d| d.name() == claim.detector)
                    || claim.protocol == self.name
            }
            None => false,
        }
    }

    /// Whether this handler is the catch-all fallback (owns any connection).
    pub fn is_default(&self) -> bool {
        self.default
    }

    /// Mark this handler as the catch-all fallback.
    pub fn with_default(mut self, yes: bool) -> Self {
        self.default = yes;
        self
    }

    /// Whether connections served by this handler get a buffered writer.
    ///
    /// HTTP/1.1 opts in (its responses are assembled across several writes);
    /// raw TCP/UDP and HTTP/2 leave it off. The server reads this flag instead
    /// of knowing which protocol it is dispatching.
    pub fn buffered_writer(&self) -> bool {
        self.buf_writer
    }

    /// Request a buffered writer for connections served by this handler.
    pub fn with_buffered_writer(mut self, yes: bool) -> Self {
        self.buf_writer = yes;
        self
    }

    /// Whether this handler serves datagrams (UDP) instead of streams.
    pub fn is_datagram(&self) -> bool {
        self.datagram
    }

    /// Mark this handler as a datagram protocol (UDP).
    pub fn with_datagram(mut self, yes: bool) -> Self {
        self.datagram = yes;
        self
    }

    /// The detector that claims connections for this handler, if any.
    pub fn detector(&self) -> Option<&Arc<dyn ProtocolDetector>> {
        self.detector.as_ref()
    }

    /// The protocol middleware chain (same [`Executor`] format as the router).
    pub fn middleware(&self) -> &[Arc<Executor>] {
        &self.middleware
    }

    /// The recursive child handlers.
    pub fn sub_handlers(&self) -> &[Arc<ProtocolHandler<C>>] {
        &self.sub_handlers
    }

    /// The protocol command registry.
    pub fn commands(&self) -> &CommandRegistry<C> {
        &self.commands
    }

    /// Mutable access to the command registry, for post-construction fills.
    pub fn commands_mut(&mut self) -> &mut CommandRegistry<C> {
        &mut self.commands
    }

    /// Attach a detector claiming connections for this protocol.
    pub fn with_detector(mut self, detector: Arc<dyn ProtocolDetector>) -> Self {
        self.detector = Some(detector);
        self
    }

    /// Append one protocol middleware (an [`Executor`]).
    pub fn with_middleware(mut self, middleware: Arc<Executor>) -> Self {
        self.middleware.push(middleware);
        self
    }

    /// Append a recursive sub-handler.
    pub fn with_sub_handler(mut self, sub: Arc<ProtocolHandler<C>>) -> Self {
        self.sub_handlers.push(sub);
        self
    }

    /// Register a protocol command handler.
    pub fn with_command<K: Into<CommandKey>>(mut self, key: K, handler: CommandHandler<C>) -> Self {
        self.commands.register(key, handler);
        self
    }

    /// Find a sub-handler by name.
    pub fn find_sub_handler(&self, name: &str) -> Option<Arc<ProtocolHandler<C>>> {
        self.sub_handlers
            .iter()
            .find(|h| h.name == name)
            .map(Arc::clone)
    }

    /// Run this handler over `ctx`:
    ///
    /// 1. record the handler in the connection's [`HandlerState`];
    /// 2. run the middleware chain — a middleware returning `false` ends the
    ///    connection without invoking the handler;
    /// 3. invoke the handler; a [`Outcome::Handoff`] descends into the named
    ///    sub-handler (which recurses through this same routine).
    pub fn run<'a>(&'a self, ctx: &'a mut Context) -> BoxFuture<'a, Outcome> {
        Box::pin(async move {
            if let Some(state) = ctx.local.get_mut::<HandlerState>() {
                state.enter(&self.name);
            }

            for mw in &self.middleware {
                let f: &Executor = &**mw;
                if !f(ctx).await {
                    return Outcome::Done;
                }
            }

            match self.handler.call(self, ctx).await {
                Outcome::Handoff(name) => match self.find_sub_handler(&name) {
                    Some(sub) => sub.run(ctx).await,
                    None => {
                        tracing::warn!(
                            "[handler] `{}` handed off to unknown sub-handler `{}`",
                            self.name,
                            name
                        );
                        Outcome::Done
                    }
                },
                other => other,
            }
        })
    }

    /// Convenience: box this handler for registration.
    pub fn shared(self) -> Arc<Self> {
        Arc::new(self)
    }
}

/// [`ProtocolHandler`] is the reference (and default) [`Protocol`]
/// implementation: every concrete protocol is built as one.
impl Protocol for ProtocolHandler<Bytes> {
    fn name(&self) -> &str {
        ProtocolHandler::name(self)
    }

    fn detector(&self) -> Option<Arc<dyn ProtocolDetector>> {
        self.detector.clone()
    }

    fn middleware(&self) -> &[Arc<Executor>] {
        ProtocolHandler::middleware(self)
    }

    fn sub_handlers(&self) -> &[Arc<ProtocolHandler<Bytes>>] {
        ProtocolHandler::sub_handlers(self)
    }

    fn commands(&self) -> &CommandRegistry<Bytes> {
        ProtocolHandler::commands(self)
    }

    fn transport(&self) -> Transport {
        if self.datagram {
            Transport::Datagram
        } else {
            Transport::Stream
        }
    }

    fn is_default(&self) -> bool {
        ProtocolHandler::is_default(self)
    }

    fn buffered_writer(&self) -> bool {
        ProtocolHandler::buffered_writer(self)
    }

    fn is_mine(&self, claim: Option<&Claim>) -> bool {
        ProtocolHandler::is_mine(self, claim)
    }

    fn run<'a>(&'a self, ctx: &'a mut Context) -> BoxFuture<'a, Outcome> {
        ProtocolHandler::run(self, ctx)
    }
}
