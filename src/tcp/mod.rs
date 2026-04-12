//! # TCP Module
//!
//! TCP protocol support for the server.
//!
//! ## Components
//!
//! - `router`: Command-based TCP router with frame validation
//! - `types`: Frame and Command traits, RawCodec implementation
//! - `listeners`: TCP connection listeners
//! - `macros`: TCP routing macros

pub mod listeners;
pub mod macros;
pub mod router;
pub mod types;
