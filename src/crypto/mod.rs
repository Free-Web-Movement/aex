//! # Crypto Module
//!
//! Cryptographic utilities for secure communication.
//!
//! ## Components
//!
//! - `zero_trust_session_key`: X25519 key exchange + ChaCha20Poly1305 encryption
//! - `session_key_manager`: Session key lifecycle management
//!
//! ## Security Model
//!
//! Uses zero-trust architecture with:
//! - X25519 for key exchange (Curve25519 ECDH)
//! - ChaCha20Poly1305 for authenticated encryption

pub mod session_key_manager;
pub mod zero_trust_session_key;
