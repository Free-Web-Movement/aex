//! # Protocol
//!
//! The uniform [`Protocol`] interface every protocol implements, plus the
//! [`Transport`] kind it serves. Protocol-specific data and methods live on the
//! concrete structs under [`crate::protocols`].

use std::sync::Arc;

use bytes::Bytes;
use futures::future::BoxFuture;

use crate::connection::context::Context;
use crate::detect::{Claim, DetectionState, ProtocolDetector};
use crate::handler::{CommandRegistry, HandlerState, Outcome};
use crate::http::router::Router as HttpRouter;
use crate::http::types::Executor;

/// What kind of transport a protocol serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    /// Connection-oriented (TCP): dispatched through detection + `is_mine`.
    Stream,
    /// Datagram-oriented (UDP): every datagram is handed to `run` directly.
    Datagram,
}

/// The one uniform interface every protocol implements.
///
/// The **trait carries the uniform capabilities** — routing ([`router`]),
/// [command handling](Self::commands), [middleware](Self::middleware),
/// recursive sub-handlers, transport flags, claim resolution and the shared
/// [`run`](Self::run) machinery. The **concrete struct carries the
/// protocol-specific data and methods** (its router, handler closures,
/// authorizer, …): protocol differences are realized by defining a struct per
/// protocol, not by a config layer.
///
/// A protocol is therefore:
///
/// - a **detector** — how stream connections are claimed for it;
/// - a **router** — HTTP-style request routing, when the protocol serves HTTP;
/// - **middleware** — the shared [`Executor`] chain (returning `false`
///   short-circuits the handler);
/// - **sub-handlers** — child protocols it can hand a connection off to
///   (HTTP → WebSocket, …);
/// - **command handlers** — decoded commands dispatched by name or id.
///
/// The server knows none of these details: it only asks `is_mine` and calls
/// `run`. The struct only tells the trait `process` — how to serve a claimed
/// connection — and everything else comes from the uniform defaults.
pub trait Protocol: Send + Sync {
    /// Unique name / protocol label.
    fn name(&self) -> &str;

    /// The protocol's own detector, if it wants connections claimed for it.
    fn detector(&self) -> Option<Arc<dyn ProtocolDetector>> {
        None
    }

    /// The router serving HTTP-style requests, if this protocol routes.
    fn router(&self) -> Option<Arc<HttpRouter>> {
        None
    }

    /// Middleware run before the handler; a middleware returning `false`
    /// short-circuits the handler.
    fn middleware(&self) -> &[Arc<Executor>] {
        &[]
    }

    /// Recursive sub-protocol handlers (HTTP → WebSocket, …).
    fn sub_handlers(&self) -> &[Arc<dyn Protocol>] {
        &[]
    }

    /// Command handlers, dispatched by name or numeric id.
    fn commands(&self) -> &CommandRegistry<Bytes>;

    /// Transport this protocol serves.
    fn transport(&self) -> Transport {
        Transport::Stream
    }

    /// Whether this is the catch-all fallback (accepts any stream connection).
    fn is_default(&self) -> bool {
        false
    }

    /// Whether the stream writer should be buffered.
    fn buffered_writer(&self) -> bool {
        false
    }

    /// Whether this protocol owns the connection described by `claim`.
    ///
    /// The default is stated purely in terms of this protocol's own name and
    /// detector, so no protocol needs to reimplement it.
    fn is_mine(&self, claim: Option<&Claim>) -> bool {
        if self.is_default() {
            return true;
        }
        match claim {
            Some(claim) => {
                self.detector().is_some_and(|d| d.name() == claim.detector)
                    || claim.protocol == self.name()
            }
            None => false,
        }
    }

    /// Protocol-specific processing of one connection (stream) or datagram.
    ///
    /// This is the only method a concrete struct must implement: the shared
    /// [`run`](Self::run) wraps it with the handler-state record, the middleware
    /// chain and sub-handler handoff resolution.
    fn process<'a>(&'a self, ctx: &'a mut Context) -> BoxFuture<'a, Outcome>;

    /// Serve one connection (stream or datagram).
    ///
    /// Uniform machinery shared by every protocol: record the handler in the
    /// connection's [`HandlerState`], run the middleware chain (a middleware
    /// returning `false` ends the connection without invoking `process`), then
    /// run [`process`](Self::process); an [`Outcome::Handoff`] descends into the
    /// named sub-handler (which recurses through this same routine).
    fn run<'a>(&'a self, ctx: &'a mut Context) -> BoxFuture<'a, Outcome> {
        Box::pin(async move {
            if let Some(state) = ctx.local.get_mut::<HandlerState>() {
                state.enter(self.name());
            }

            for mw in self.middleware() {
                let f: &Executor = &**mw;
                if !f(ctx).await {
                    return Outcome::Done;
                }
            }

            // Nested dispatch: a top-level handler (TCP/UDP) owns every
            // connection of its transport, then hands it down to the child
            // that actually claims the detected protocol (TCP → HTTP →
            // WebSocket, …). Ownership is decided exactly as at the server,
            // by `is_mine` on the detection claim.
            if let Some(claim) = ctx
                .local
                .get_ref::<DetectionState>()
                .and_then(|s| s.claim().cloned())
            {
                if let Some(sub) = self.sub_handlers().iter().find(|h| h.is_mine(Some(&claim))) {
                    return sub.run(ctx).await;
                }
            }

            match self.process(ctx).await {
                Outcome::Handoff(name) => {
                    match self.sub_handlers().iter().find(|h| h.name() == name) {
                        Some(sub) => sub.run(ctx).await,
                        None => {
                            tracing::warn!(
                                "[handler] `{}` handed off to unknown sub-handler `{}`",
                                self.name(),
                                name
                            );
                            Outcome::Done
                        }
                    }
                }
                other => other,
            }
        })
    }
}
