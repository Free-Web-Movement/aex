//! Concrete protocols, each a [`Protocol`] built from the uniform building
//! blocks (detector + handler + router + middleware + sub-handlers + commands).
//!
//! Nothing here is known to the [`server`](crate::server) — a protocol bundles
//! its own detector, its own router/handler (with any sub-handlers and command
//! handlers), and the server only asks [`is_mine`](crate::protocol::Protocol::is_mine)
//! then dispatches to [`run`](crate::protocol::Protocol::run).

pub mod classify;
pub mod http;
pub mod http1;
pub mod http2;
mod macros;
#[cfg(feature = "proxy")]
pub mod socks;
pub mod tcp;
pub mod udp;
pub mod websocket;

#[cfg(feature = "proxy")]
pub mod proxy;
