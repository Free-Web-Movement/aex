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
//! use aex::unified::Server;
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
//!     Server::solo(addr)
//!         .push(tcp!(http!(router)))
//!         .start()
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
pub mod http;
pub mod http2;
pub mod macros;
pub mod nat;
pub mod protocols;
#[cfg(feature = "proxy")]
pub mod proxy;
pub mod storage;
pub mod tcp;
pub mod time;
pub mod udp;
pub mod unified;

pub use aex_macros::routes;

#[doc(hidden)]
pub use futures::future::FutureExt;
