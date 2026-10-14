//! SOCKS4/4a/5 proxy protocol handler.
//!
//! Owns its detector ([`crate::proxy::SocksDetector`]) and wraps the relay
//! handler from the `proxy` module so it mounts through the same uniform
//! [`Protocol`] machinery as every other protocol.

use std::sync::Arc;

use bytes::Bytes;

use crate::connection::context::Context;
use crate::detect::ProtocolDetector;
use crate::handler::{CommandHandler, CommandKey, CommandRegistry, Outcome, ProtocolHandler};
use crate::protocol::{Protocol, Transport};
use crate::protocols::tcp::{TCPHandler, from_handler};
use crate::proxy::{ProxyAuthorizer, SocksDetector, socks_tcp_handler};

/// SOCKS proxy protocol: claims connections via its detector and relays them.
pub struct Socks {
    handler: TCPHandler,
    commands: CommandRegistry<Bytes>,
}

impl Socks {
    /// The SOCKS proxy protocol. `authorizer` is the shared credential check;
    /// `None` runs an open relay.
    pub fn new(authorizer: Option<ProxyAuthorizer>) -> Self {
        Self {
            handler: socks_tcp_handler(authorizer),
            commands: CommandRegistry::new(),
        }
    }

    /// Register a protocol command handler.
    pub fn with_command<K: Into<CommandKey>>(
        mut self,
        key: K,
        handler: CommandHandler<Bytes>,
    ) -> Self {
        self.commands.register(key, handler);
        self
    }

    /// Serve one SOCKS connection (struct-specific processing).
    pub async fn handle(&self, ctx: &mut Context) -> Outcome {
        let owned = crate::handler::take_context(ctx);
        (self.handler)(owned);
        Outcome::Done
    }

    /// Box this protocol for registration.
    pub fn shared(self) -> Arc<Self> {
        Arc::new(self)
    }
}

impl Protocol for Socks {
    fn name(&self) -> &str {
        "socks"
    }

    fn detector(&self) -> Option<Arc<dyn ProtocolDetector>> {
        Some(Arc::new(SocksDetector))
    }

    fn commands(&self) -> &CommandRegistry<Bytes> {
        &self.commands
    }

    fn transport(&self) -> Transport {
        Transport::Stream
    }

    fn process<'a>(&'a self, ctx: &'a mut Context) -> futures::future::BoxFuture<'a, Outcome> {
        Box::pin(async move { self.handle(ctx).await })
    }
}

/// Backward-compatible convenience: build the SOCKS [`ProtocolHandler`],
/// carrying its detector.
pub fn protocol(authorizer: Option<ProxyAuthorizer>) -> Arc<ProtocolHandler<Bytes>> {
    let sock = from_handler("socks", Some(socks_tcp_handler(authorizer)));
    let sock = Arc::into_inner(sock).expect("freshly built handler has no other owner");
    Arc::new(sock.with_detector(Arc::new(SocksDetector)))
}
