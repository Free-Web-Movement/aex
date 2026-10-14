//! # P2P Module
//!
//! Peer-to-peer transport-layer building blocks provided by aex. Currently this
//! hosts NAT traversal support ([`nat`]), which operates directly on sockets and
//! stays below the `connection` layer.

pub mod nat;
