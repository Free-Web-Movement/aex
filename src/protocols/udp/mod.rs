//! # UDP Module
//!
//! UDP protocol support for the server.
//!
//! ## Components
//!
//! - `router`: Packet-based UDP router with per-packet spawning
//! - `types`: UDP-specific type definitions

pub mod protocol;
pub mod router;
pub mod types;

pub use protocol::*;
