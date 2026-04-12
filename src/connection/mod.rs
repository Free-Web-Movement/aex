//! # Connection Module
//!
//! Connection management for HTTP/TCP/UDP protocols.
//!
//! ## Components
//!
//! - `context`: Per-request Context with TypeMap storage
//! - `global`: GlobalContext for shared state across connections
//! - `manager`: ConnectionManager for connection pool lifecycle
//! - `entry`: ConnectionEntry for individual connection metadata
//! - `node`: Node representation and IP filtering
//! - `scope`: NetworkScope (LAN/WAN) classification
//! - `status`: Connection status tracking
//! - `protocol`: Protocol type definitions
//! - `types`: Connection-related type definitions

pub mod context;
pub mod entry;
pub mod global;
pub mod manager;
pub mod node;
pub mod protocol;
pub mod scope;
pub mod status;
pub mod types;
