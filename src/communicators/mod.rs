//! # Communicators Module
//!
//! Inter-Process Communication (IPC) patterns.
//!
//! ## Components
//!
//! - `spreader`: Pub/Sub broadcast (1:N communication)
//! - `event`: Event system for M:N communication
//! - `pipe`: Named pipes for 1:1 communication

pub mod event;
pub mod pipe;
pub mod spreader;
