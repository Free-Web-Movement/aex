//! SOCKS4/4a/5 proxy protocol handler.
//!
//! Owns its detector ([`crate::proxy::SocksDetector`]) and wraps the relay
//! handler from the `proxy` module so it mounts through the same uniform
//! [`Protocol`] machinery as every other protocol.

use std::sync::Arc;

use bytes::Bytes;

use crate::protocols::tcp::from_handler;
use crate::proxy::{ProxyAuthorizer, SocksDetector, socks_tcp_handler};
use crate::unified::handler::ProtocolHandler;

/// Build the SOCKS proxy [`ProtocolHandler`], carrying its detector.
pub fn protocol(authorizer: Option<ProxyAuthorizer>) -> Arc<ProtocolHandler<Bytes>> {
    let sock = from_handler("socks", Some(socks_tcp_handler(authorizer)));
    let sock = Arc::into_inner(sock).expect("freshly built handler has no other owner");
    Arc::new(sock.with_detector(Arc::new(SocksDetector)))
}
