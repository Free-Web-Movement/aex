//! Raw TCP fallback protocol handler.
//!
//! Used for connections no detector claims, and as the wrapper for
//! user-registered `custom_handler`s. The user-facing closure takes ownership of
//! the [`Context`], so the handler reconstructs an owned context from the one
//! the runtime hands it.

use std::sync::Arc;

use bytes::Bytes;

use crate::connection::context::Context;
use crate::unified::handler::{Outcome, ProtocolHandler, take_context};

/// Raw TCP connection handler (takes ownership of the [`Context`]).
pub type TCPHandler = Arc<dyn Fn(Context) -> tokio::task::JoinHandle<()> + Send + Sync>;

/// Configuration for the raw TCP handler.
#[derive(Clone, Default)]
pub struct TcpConfig {
    /// The user TCP handler; when absent the connection is dropped.
    pub handler: Option<TCPHandler>,
}

/// Build the default raw TCP [`ProtocolHandler`] (named `"tcp"`).
///
/// This is the catch-all fallback: it accepts any connection no other handler
/// claims.
pub fn protocol(cfg: TcpConfig) -> Arc<ProtocolHandler<Bytes>> {
    Arc::new(build("tcp", cfg.handler).with_default(true))
}

/// Build a [`ProtocolHandler`] named `name` wrapping a user [`TCPHandler`].
///
/// Custom protocols claimed by a detector are dispatched through this, so they
/// get the same state record, middleware and recursive machinery as built-ins.
pub fn from_handler(
    name: impl Into<String>,
    handler: Option<TCPHandler>,
) -> Arc<ProtocolHandler<Bytes>> {
    Arc::new(build(name, handler))
}

fn build(name: impl Into<String>, handler: Option<TCPHandler>) -> ProtocolHandler<Bytes> {
    ProtocolHandler::new(
        name,
        move |_handler: &ProtocolHandler<Bytes>, ctx: &mut Context| {
            let handler = handler.clone();
            Box::pin(async move {
                match handler {
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
            })
        },
    )
}
