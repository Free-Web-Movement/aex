//! UDP protocol handler.
//!
//! Every datagram builds a short-lived [`Context`] carrying the raw payload; the
//! handler reconstructs an owned context for the user-facing closure.

use std::sync::Arc;

use bytes::Bytes;

use crate::connection::context::Context;
use crate::unified::handler::{Outcome, ProtocolHandler, take_context};

/// UDP datagram handler (takes ownership of the [`Context`]).
pub type UDPHandler = Arc<dyn Fn(Context) -> tokio::task::JoinHandle<()> + Send + Sync>;

/// Configuration for the UDP handler.
#[derive(Clone, Default)]
pub struct UdpConfig {
    /// The user UDP handler; when absent datagrams are dropped.
    pub handler: Option<UDPHandler>,
}

/// Build the UDP [`ProtocolHandler`].
pub fn protocol(cfg: UdpConfig) -> Arc<ProtocolHandler<Bytes>> {
    let handler = cfg.handler.clone();
    Arc::new(
        ProtocolHandler::new(
            "udp",
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
                                "[Unified] No UDP handler registered, dropping datagram"
                            );
                        }
                    }
                    Outcome::Done
                })
            },
        )
        .with_datagram(true),
    )
}
