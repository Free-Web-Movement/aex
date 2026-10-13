//! # Unified Protocol Server
//!
//! A transport-generic server. It accepts TCP connections (and optionally UDP
//! datagrams) on a single port and offers each one to the mounted protocols;
//! the first protocol that claims it serves it. Everything protocol-specific —
//! detectors, routers, request parsing — lives in the protocol implementations
//! under [`crate::protocols`], never here.
//!
//! ```rust,ignore
//! use aex::{http, tcp};
//!
//! Server::new(globals)
//!     .push(tcp!(http!(router)))
//!     .listen(addr).await?;
//! ```

use std::net::SocketAddr;
use std::os::fd::AsRawFd;
use std::sync::Arc;

use bytes::Bytes;
use futures::future::BoxFuture;
use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, TcpStream, UdpSocket};

pub mod detect;
pub mod handler;
pub use detect::{
    Claim, DetectionEvent, DetectionState, DetectorMode, DetectorRegistry, MAX_PEEK, Position,
    ProtocolDetector, RegisterError, Verdict, run_pipeline,
};

use crate::connection::context::{BoxReader, BoxWriter, ConnectionFd, Context};
use crate::http::router::Router as HttpRouter;
use crate::http::types::Executor;
use crate::unified::handler::{CommandRegistry, HandlerState, Outcome};

/// What kind of transport a protocol serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    /// Connection-oriented (TCP): dispatched through detection + `is_mine`.
    Stream,
    /// Datagram-oriented (UDP): every datagram is handed to `run` directly.
    Datagram,
}

/// The one uniform interface every protocol implements.
///
/// The **trait carries the uniform capabilities** — routing ([`router`]),
/// [command handling](Self::commands), [middleware](Self::middleware),
/// recursive sub-handlers, transport flags, claim resolution and the shared
/// [`run`](Self::run) machinery. The **concrete struct carries the
/// protocol-specific data and methods** (its router, handler closures,
/// authorizer, …): protocol differences are realized by defining a struct per
/// protocol, not by a config layer.
///
/// A protocol is therefore:
///
/// - a **detector** — how stream connections are claimed for it;
/// - a **router** — HTTP-style request routing, when the protocol serves HTTP;
/// - **middleware** — the shared [`Executor`] chain (returning `false`
///   short-circuits the handler);
/// - **sub-handlers** — child protocols it can hand a connection off to
///   (HTTP → WebSocket, …);
/// - **command handlers** — decoded commands dispatched by name or id.
///
/// The server knows none of these details: it only asks `is_mine` and calls
/// `run`. The struct only tells the trait `process` — how to serve a claimed
/// connection — and everything else comes from the uniform defaults.
pub trait Protocol: Send + Sync {
    /// Unique name / protocol label.
    fn name(&self) -> &str;

    /// The protocol's own detector, if it wants connections claimed for it.
    fn detector(&self) -> Option<Arc<dyn ProtocolDetector>> {
        None
    }

    /// The router serving HTTP-style requests, if this protocol routes.
    fn router(&self) -> Option<Arc<HttpRouter>> {
        None
    }

    /// Middleware run before the handler; a middleware returning `false`
    /// short-circuits the handler.
    fn middleware(&self) -> &[Arc<Executor>] {
        &[]
    }

    /// Recursive sub-protocol handlers (HTTP → WebSocket, …).
    fn sub_handlers(&self) -> &[Arc<dyn Protocol>] {
        &[]
    }

    /// Command handlers, dispatched by name or numeric id.
    fn commands(&self) -> &CommandRegistry<Bytes>;

    /// Transport this protocol serves.
    fn transport(&self) -> Transport {
        Transport::Stream
    }

    /// Whether this is the catch-all fallback (accepts any stream connection).
    fn is_default(&self) -> bool {
        false
    }

    /// Whether the stream writer should be buffered.
    fn buffered_writer(&self) -> bool {
        false
    }

    /// Whether this protocol owns the connection described by `claim`.
    ///
    /// The default is stated purely in terms of this protocol's own name and
    /// detector, so no protocol needs to reimplement it.
    fn is_mine(&self, claim: Option<&Claim>) -> bool {
        if self.is_default() {
            return true;
        }
        match claim {
            Some(claim) => {
                self.detector().is_some_and(|d| d.name() == claim.detector)
                    || claim.protocol == self.name()
            }
            None => false,
        }
    }

    /// Protocol-specific processing of one connection (stream) or datagram.
    ///
    /// This is the only method a concrete struct must implement: the shared
    /// [`run`](Self::run) wraps it with the handler-state record, the middleware
    /// chain and sub-handler handoff resolution.
    fn process<'a>(&'a self, ctx: &'a mut Context) -> BoxFuture<'a, Outcome>;

    /// Serve one connection (stream or datagram).
    ///
    /// Uniform machinery shared by every protocol: record the handler in the
    /// connection's [`HandlerState`], run the middleware chain (a middleware
    /// returning `false` ends the connection without invoking `process`), then
    /// run [`process`](Self::process); an [`Outcome::Handoff`] descends into the
    /// named sub-handler (which recurses through this same routine).
    fn run<'a>(&'a self, ctx: &'a mut Context) -> BoxFuture<'a, Outcome> {
        Box::pin(async move {
            if let Some(state) = ctx.local.get_mut::<HandlerState>() {
                state.enter(self.name());
            }

            for mw in self.middleware() {
                let f: &Executor = &**mw;
                if !f(ctx).await {
                    return Outcome::Done;
                }
            }

            // Nested dispatch: a top-level handler (TCP/UDP) owns every
            // connection of its transport, then hands it down to the child
            // that actually claims the detected protocol (TCP → HTTP →
            // WebSocket, …). Ownership is decided exactly as at the server,
            // by `is_mine` on the detection claim.
            if let Some(claim) = ctx
                .local
                .get_ref::<DetectionState>()
                .and_then(|s| s.claim().cloned())
            {
                if let Some(sub) = self
                    .sub_handlers()
                    .iter()
                    .find(|h| h.is_mine(Some(&claim)))
                {
                    return sub.run(ctx).await;
                }
            }

            match self.process(ctx).await {
                Outcome::Handoff(name) => match self
                    .sub_handlers()
                    .iter()
                    .find(|h| h.name() == name)
                {
                    Some(sub) => sub.run(ctx).await,
                    None => {
                        tracing::warn!(
                            "[handler] `{}` handed off to unknown sub-handler `{}`",
                            self.name(),
                            name
                        );
                        Outcome::Done
                    }
                },
                other => other,
            }
        })
    }
}

/// The unified server. Protocol-agnostic by construction.
pub struct Server {
    pub addr: SocketAddr,
    pub globals: Arc<crate::connection::global::GlobalContext>,
    /// Ordered protocol detectors consulted before dispatching a stream.
    pub registry: Arc<DetectorRegistry>,
    /// Mounted stream protocols; the first whose `is_mine` accepts a
    /// connection serves it. Default protocols are kept last.
    pub protocols: Vec<Arc<dyn Protocol>>,
    /// The datagram protocol, if any (UDP).
    pub datagram: Option<Arc<dyn Protocol>>,
    /// Master switch for the detection phase; when off, stream connections go
    /// straight to the fallback protocol without any peeking.
    pub detect_enabled: bool,
    #[doc(hidden)]
    pub _udp_socket: Option<UdpSocket>,
}

/// Backward-compatible alias for [`Server`].
pub type UnifiedServer = Server;

impl Server {
    /// Create an empty server sharing `globals`, with no address yet.
    ///
    /// Mount protocols with [`Server::push`] and serve with [`Server::listen`]:
    /// `Server::new(gctx).push(p1).push(p2).listen(addr).await?`.
    pub fn new(globals: Arc<crate::connection::global::GlobalContext>) -> Self {
        Self::at(SocketAddr::from(([0, 0, 0, 0], 0)), globals)
    }

    /// Create a server bound to `addr` (legacy form; [`Server::listen`]
    /// overrides the address).
    pub fn at(addr: SocketAddr, globals: Arc<crate::connection::global::GlobalContext>) -> Self {
        Self {
            addr,
            globals,
            registry: Arc::new(DetectorRegistry::new()),
            protocols: Vec::new(),
            datagram: None,
            detect_enabled: true,
            _udp_socket: None,
        }
    }

    /// Create a self-contained server bound to `addr` with a fresh
    /// [`GlobalContext`] (including a new paired session key), for examples and
    /// tests that do not share a context with the rest of an application.
    pub fn solo(addr: SocketAddr) -> Self {
        let globals = Arc::new(crate::connection::global::GlobalContext::new(
            addr,
            Some(Arc::new(tokio::sync::Mutex::new(
                crate::crypto::session_key_manager::PairedSessionKey::new(16),
            ))),
        ));
        Self::at(addr, globals)
    }

    /// Mount a protocol. This is the single entry point used by every protocol.
    pub fn push(mut self, protocol: Arc<dyn Protocol>) -> Self {
        self.mount(protocol);
        self
    }

    /// Mount a protocol in place.
    ///
    /// Detectors are registered for the protocol **and, recursively, for every
    /// sub-handler**, because the top level only ever mounts the transport
    /// handlers ([`Tcp`](crate::protocols::tcp::Tcp) /
    /// [`Udp`](crate::protocols::udp::Udp)); everything else (HTTP, WebSocket,
    /// SOCKS, NAT, …) is nested beneath them and reached by handoff.
    pub fn mount(&mut self, protocol: Arc<dyn Protocol>) {
        self.register_detectors(&protocol);

        if protocol.transport() == Transport::Datagram {
            self.datagram = Some(protocol);
            return;
        }

        // Keep the catch-all stream handler (TCP) last so a directly mounted
        // claiming protocol still wins; a TCP carrying sub-handlers owns
        // dispatch itself and hands down to its children.
        self.protocols.retain(|p| p.name() != protocol.name());
        if protocol.is_default() {
            self.protocols.push(protocol);
        } else {
            let pos = self
                .protocols
                .iter()
                .position(|p| p.is_default())
                .unwrap_or(self.protocols.len());
            self.protocols.insert(pos, protocol);
        }
    }

    /// Register `protocol`'s detector (if any) and recurse into its children.
    fn register_detectors(&mut self, protocol: &Arc<dyn Protocol>) {
        if let Some(detector) = protocol.detector() {
            let name = detector.name().to_string();
            if !self.registry.list().iter().any(|n| *n == name) {
                if let Err(e) = self.registry.register_at(Position::Back, detector) {
                    tracing::warn!("[Unified] detector registration failed: {}", e);
                }
            }
        }
        for sub in protocol.sub_handlers() {
            self.register_detectors(sub);
        }
    }

    /// The top-level stream handler (TCP), which every stream connection is
    /// dispatched to and which hands down to the claiming child.
    fn stream_root(&self) -> Option<Arc<dyn Protocol>> {
        self.protocols
            .iter()
            .find(|p| p.transport() == Transport::Stream && p.is_default())
            .cloned()
    }

    /// Resolve whether the claiming protocol wants a buffered writer, looking
    /// down the root's child tree.
    fn resolve_buffered_writer(&self, claim: Option<&Claim>) -> bool {
        fn search(p: &Arc<dyn Protocol>, claim: Option<&Claim>) -> Option<bool> {
            for sub in p.sub_handlers() {
                if !sub.is_default() && sub.is_mine(claim) {
                    return Some(sub.buffered_writer());
                }
                if let Some(v) = search(sub, claim) {
                    return Some(v);
                }
            }
            None
        }

        let Some(root) = self.stream_root() else {
            return false;
        };
        search(&root, claim).unwrap_or_else(|| root.buffered_writer())
    }

    /// Register a protocol detector at the back of the detection pipeline.
    /// Registration errors (duplicate name, conflict) are logged, not fatal.
    pub fn detector(self, d: Arc<dyn ProtocolDetector>) -> Self {
        self.detector_at(Position::Back, d)
    }

    /// Register a protocol detector at an explicit position.
    pub fn detector_at(self, pos: Position, d: Arc<dyn ProtocolDetector>) -> Self {
        if let Err(e) = self.registry.register_at(pos, d) {
            tracing::warn!("[Unified] detector registration failed: {}", e);
        }
        self
    }

    /// Share an externally-managed registry, e.g. one mutated at runtime by
    /// other parts of the application.
    pub fn with_registry(mut self, registry: Arc<DetectorRegistry>) -> Self {
        self.registry = registry;
        self
    }

    /// Enable/disable the detection phase entirely. When disabled, TCP
    /// connections go straight to the fallback without peeking.
    pub fn detection(mut self, enabled: bool) -> Self {
        self.detect_enabled = enabled;
        self
    }

    pub async fn handle_tcp_connection(&self, mut socket: TcpStream, peer_addr: SocketAddr) {
        // Detection phase: buffer bytes and run the detector pipeline until
        // some detector claims the connection, every pending detector has
        // passed, or the peek cap is reached. With an empty registry (or
        // detection disabled) nothing is read here at all.
        let detectors = self.registry.snapshot();
        let mut initial_data: Vec<u8> = Vec::with_capacity(256);
        let mut state = DetectionState::new();

        if self.detect_enabled && !detectors.is_empty() {
            let mut chunk = [0u8; 2048];
            loop {
                match socket.read(&mut chunk).await {
                    Ok(0) | Err(_) => {
                        if initial_data.is_empty() {
                            return;
                        }
                        state.finish();
                        break;
                    }
                    Ok(n) => initial_data.extend_from_slice(&chunk[..n]),
                }
                run_pipeline(&detectors, &initial_data, &mut state);
                if state.is_finished() || initial_data.len() >= MAX_PEEK {
                    break;
                }
            }
        }

        let claim = state.claim().cloned();

        // Ownership is decided by `is_mine`. A top-level TCP carrying
        // sub-handlers is the catch-all and hands the connection down to the
        // child that claims the detected protocol; otherwise the claiming
        // protocol mounted directly is dispatched.
        let chosen = self
            .protocols
            .iter()
            .find(|p| p.is_mine(claim.as_ref()))
            .cloned();

        match chosen {
            Some(protocol) => {
                let buf_writer = if protocol.is_default() && !protocol.sub_handlers().is_empty() {
                    self.resolve_buffered_writer(claim.as_ref())
                } else {
                    protocol.buffered_writer()
                };
                let mut ctx =
                    self.build_stream_ctx(socket, peer_addr, initial_data, buf_writer, Some(state));
                protocol.run(&mut ctx).await;
            }
            None => {
                tracing::warn!(
                    "[Unified] no protocol handler for connection from {}",
                    peer_addr
                );
            }
        }
    }

    /// Build a stream [`Context`] from an accepted socket plus any bytes already
    /// read during detection, installing the connection fd and a fresh
    /// [`HandlerState`] record. `buf_writer` wraps the writer in a
    /// [`tokio::io::BufWriter`].
    fn build_stream_ctx(
        &self,
        socket: TcpStream,
        peer_addr: SocketAddr,
        initial_data: Vec<u8>,
        buf_writer: bool,
        detection: Option<DetectionState>,
    ) -> Context {
        let fd = socket.as_raw_fd();
        let local_addr = socket.local_addr().ok();
        let (reader, writer) = socket.into_split();
        let cursor = std::io::Cursor::new(initial_data);
        let reader_with_buf = tokio::io::BufReader::new(cursor.chain(reader));

        let reader: BoxReader = Box::new(reader_with_buf);
        let writer: BoxWriter = if buf_writer {
            Box::new(tokio::io::BufWriter::new(writer))
        } else {
            Box::new(writer)
        };

        let mut ctx = Context::new(Some(reader), Some(writer), self.globals.clone(), peer_addr);
        ctx.local_addr = local_addr;
        ctx.local.set_value(ConnectionFd(fd));
        ctx.local.set_value(HandlerState::new());
        if let Some(state) = detection {
            ctx.local.set_value(state);
        }
        ctx
    }

    pub async fn start(&self) -> anyhow::Result<()> {
        let tcp_listener = TcpListener::bind(self.addr).await?;
        tracing::info!("[Unified] TCP listening on {}", self.addr);

        let self_arc = Arc::new(self.clone());

        if let Some(datagram) = &self_arc.datagram {
            let sock = Arc::new(UdpSocket::bind(self_arc.addr).await?);
            tracing::info!("[Unified] UDP listening on {}", sock.local_addr()?);

            let proto = datagram.clone();
            let globals = self_arc.globals.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 65535];
                loop {
                    match sock.recv_from(&mut buf).await {
                        Ok((n, peer)) => {
                            let data = buf[..n].to_vec();
                            let mut ctx = Context::new(None, None, globals.clone(), peer);
                            ctx.set(data);
                            ctx.local.set_value(HandlerState::new());
                            proto.run(&mut ctx).await;
                        }
                        Err(e) => {
                            tracing::warn!("[Unified] UDP recv error: {}", e);
                            break;
                        }
                    }
                }
            });
        }

        loop {
            tokio::select! {
                result = tcp_listener.accept() => {
                    match result {
                        Ok((socket, peer_addr)) => {
                            let srv = self_arc.clone();
                            tokio::spawn(async move {
                                srv.handle_tcp_connection(socket, peer_addr).await;
                            });
                        }
                        Err(e) => {
                            tracing::warn!("[Unified] Accept error: {}", e);
                        }
                    }
                }
            }
        }
    }

    /// Bind `addr` and serve until the process stops.
    ///
    /// Ergonomic finish for [`Server::new`]:
    /// `Server::new(gctx).push(p1).push(p2).listen(addr).await?`.
    pub async fn listen(mut self, addr: SocketAddr) -> anyhow::Result<()> {
        self.addr = addr;
        self.start().await
    }
}


impl Clone for Server {
    fn clone(&self) -> Self {
        Self {
            addr: self.addr,
            globals: self.globals.clone(),
            registry: self.registry.clone(),
            protocols: self.protocols.clone(),
            datagram: self.datagram.clone(),
            detect_enabled: self.detect_enabled,
            _udp_socket: None,
        }
    }
}
