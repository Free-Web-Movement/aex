//! # Server
//!
//! The transport-generic unified [`Server`]. It accepts TCP connections (and
//! optionally UDP datagrams) on a single port and offers each one to the mounted
//! protocols; the first protocol that claims it serves it.

use std::net::SocketAddr;
use std::os::fd::AsRawFd;
use std::sync::Arc;

use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, TcpStream, UdpSocket};

use crate::connection::context::{BoxReader, BoxWriter, ConnectionFd, Context};
use crate::detect::{
    Claim, DetectionState, DetectorRegistry, MAX_PEEK, Position, ProtocolDetector, run_pipeline,
};
use crate::handler::HandlerState;
use crate::protocol::{Protocol, Transport};

/// The unified server. Protocol-agnostic by construction.
pub struct Server {
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

impl Default for Server {
    /// A server that owns a fresh [`GlobalContext`] and [`PairedSessionKey`],
    /// for when no context exists yet: `Server::default().push(p).start(addr)`.
    fn default() -> Self {
        let addr = SocketAddr::from(([0, 0, 0, 0], 0));
        let globals = Arc::new(crate::connection::global::GlobalContext::new(
            addr,
            Some(Arc::new(tokio::sync::Mutex::new(
                crate::crypto::session_key_manager::PairedSessionKey::new(16),
            ))),
        ));
        Self::new(globals)
    }
}

impl Server {
    /// Create an empty server sharing `globals`.
    ///
    /// Mount protocols with [`Server::push`] and serve with [`Server::start`]:
    /// `Server::new(gctx).push(p1).push(p2).start(addr).await?`.
    pub fn new(globals: Arc<crate::connection::global::GlobalContext>) -> Self {
        Self {
            globals,
            registry: Arc::new(DetectorRegistry::new()),
            protocols: Vec::new(),
            datagram: None,
            detect_enabled: true,
            _udp_socket: None,
        }
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

    /// Bind `addr` and serve until the process stops.
    ///
    /// `Server::new(gctx).push(p1).push(p2).start(addr).await?`
    /// or `Server::default().push(p).start(addr).await?`.
    pub async fn start(&self, addr: SocketAddr) -> anyhow::Result<()> {
        let tcp_listener = TcpListener::bind(addr).await?;
        tracing::info!("[Unified] TCP listening on {}", addr);

        let self_arc = Arc::new(self.clone());

        if let Some(datagram) = &self_arc.datagram {
            let sock = Arc::new(UdpSocket::bind(addr).await?);
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
}

impl Clone for Server {
    fn clone(&self) -> Self {
        Self {
            globals: self.globals.clone(),
            registry: self.registry.clone(),
            protocols: self.protocols.clone(),
            datagram: self.datagram.clone(),
            detect_enabled: self.detect_enabled,
            _udp_socket: None,
        }
    }
}
