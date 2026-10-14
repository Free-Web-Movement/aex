//! # AEX - Async-first, Executor-based Web/TCP/UDP Framework
//!
//! A lightweight, async-first Rust web framework with explicit middleware execution
//! and native WebSocket support.
//!
//! ## Core Features
//!
//! - **Intuitive HTTP Routing**: Trie-tree based router supporting static, param, and wildcard paths
//! - **Explicit Middleware Chain**: Linear execution order, predictable control flow (not onion model)
//! - **Native WebSocket**: Natural integration as middleware, shares HTTP context
//! - **Multi-Protocol**: Unified server for HTTP, TCP, and UDP
//!
//! ## Quick Example
//!
//! ```rust,ignore
//! use aex::http::router::Router as HttpRouter;
//! use aex::server::Server;
//! use aex::{http, tcp};
//! use std::net::SocketAddr;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let addr: SocketAddr = "0.0.0.0:8080".parse()?;
//!     let mut router = HttpRouter::default();
//!
//!     router.get("/", |_| "Hello, World!");
//!
//!     Server::default()
//!         .push(http!(router))
//!         .start(addr)
//!         .await?;
//!     Ok(())
//! }
//! ```
//!
//! ## Architecture
//!
//! - Server: Multi-protocol server (HTTP/TCP/UDP)
//! - Router: Trie-tree based HTTP router
//! - Executor Chain: Linear middleware + handler execution
//!
//! ## Modules
//!
//! - `http`: HTTP web framework
//! - `tcp`: TCP protocol support
//! - `udp`: UDP protocol support
//! - `connection`: Connection management
//! - `crypto`: Cryptography utilities
//! - `communicators`: IPC patterns (pub/sub, events, pipes)

pub mod communicators;
pub mod connection;
pub mod constants;
pub mod crypto;
pub mod detect;
pub mod handler;
pub mod macros;
pub mod p2p;
pub mod protocol;
pub mod protocols;
#[cfg(feature = "proxy")]
pub mod proxy;
pub mod server;
pub mod storage;
pub mod time;

// The protocol machinery now lives under `protocols/`; keep the historical
// top-level paths (`aex::http`, `aex::http2`, `aex::tcp`, `aex::udp`) working.
pub use protocols::{http, http2, tcp, udp};

// NAT now lives under `p2p/`; keep the historical `aex::nat` path working.
pub use p2p::nat;

pub use aex_macros::routes;

/// Re-export of [`bytes`] so downstream crates can name the `Bytes` type used by
/// the TCP/UDP router APIs without depending on a matching `bytes` version.
pub use bytes;

#[doc(hidden)]
pub use futures::future::FutureExt;
