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
//! use aex::http::router::{NodeType, Router as HttpRouter};
//! use aex::server::HTTPServer;
//! use aex::tcp::types::{Command, RawCodec};
//! use aex::exe;
//! use std::net::SocketAddr;
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let addr: SocketAddr = "0.0.0.0:8080".parse()?;
//!     let mut router = HttpRouter::new(NodeType::Static("root".into()));
//!
//!     router.get("/", exe!(|ctx| {
//!         ctx.send("Hello, World!");
//!         true
//!     })).register();
//!
//!     HTTPServer::new(addr, None)
//!         .http(router)
//!         .start::<RawCodec, RawCodec>(Arc::new(|c| c.id()))
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
pub mod server;
pub mod storage;
pub mod tcp;
pub mod time;
pub mod udp;

pub use server::{HttpVersions, Server, HTTPServer};
