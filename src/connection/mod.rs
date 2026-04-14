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
//! - `commands`: P2P handshake commands
//! - `heartbeat`: P2P heartbeat/keepalive
//! - `state_machine`: Connection state machine

pub mod commands;
pub mod context;
pub mod entry;
pub mod global;
pub mod handshake;
pub mod handshake_handler;
pub mod heartbeat;
pub mod manager;
pub mod node;
pub mod protocol;
pub mod scope;
pub mod state_machine;
pub mod status;
pub mod types;
