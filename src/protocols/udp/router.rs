use bytes::Bytes;
use futures::future::BoxFuture;
use std::net::SocketAddr;
use std::{collections::HashMap, sync::Arc};

use tokio::net::UdpSocket;

use crate::connection::global::GlobalContext;

/// A registered UDP handler.
///
/// Receives the global context, the already-configured `frame`/`command` bytes,
/// the peer address and the shared socket. Framing is done by the router's
/// [`Extractor`].
pub type UdpHandler = Arc<
    dyn Fn(
            Arc<GlobalContext>,
            Bytes,
            Bytes,
            SocketAddr,
            Arc<UdpSocket>,
        ) -> BoxFuture<'static, anyhow::Result<bool>>
        + Send
        + Sync
        + 'static,
>;

/// Turns a raw datagram into `(key, frame_bytes, command_bytes)`.
///
/// Returns `Ok(None)` when the datagram is not for this router, and `Err` when
/// the bytes are positively not this router's protocol.
pub type Extractor =
    Arc<dyn Fn(&[u8]) -> anyhow::Result<Option<(u32, Bytes, Bytes)>> + Send + Sync + 'static>;

/// Codec-agnostic UDP router. Each datagram is a complete message, so no
/// stream reassembly is required.
pub struct Router {
    pub handlers: HashMap<u32, UdpHandler>,
    extractor: Option<Extractor>,
}

impl Default for Router {
    fn default() -> Self {
        Self::new()
    }
}

impl Router {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
            extractor: None,
        }
    }

    /// Install the byte-level extractor that derives dispatch key plus frame and
    /// command bytes. Replaces the former `Router<F, C>` type parameters.
    pub fn extractor<E>(mut self, extractor: E) -> Self
    where
        E: Fn(&[u8]) -> anyhow::Result<Option<(u32, Bytes, Bytes)>> + Send + Sync + 'static,
    {
        self.extractor = Some(Arc::new(extractor));
        self
    }

    pub fn get_extractor(&self) -> Option<&Extractor> {
        self.extractor.as_ref()
    }

    pub fn handler_count(&self) -> usize {
        self.handlers.len()
    }

    pub fn on<FFut, Fut>(&mut self, key: u32, f: FFut)
    where
        FFut: Fn(Arc<GlobalContext>, Bytes, Bytes, SocketAddr, Arc<UdpSocket>) -> Fut
            + Send
            + Sync
            + 'static,
        Fut: Future<Output = anyhow::Result<bool>> + Send + 'static,
    {
        let handler: UdpHandler = Arc::new(move |global, frame, cmd, addr, socket| {
            Box::pin(f(global, frame, cmd, addr, socket))
        });
        self.handlers.insert(key, handler);
    }

    pub async fn handle(
        self: Arc<Self>,
        global: Arc<GlobalContext>,
        socket: Arc<UdpSocket>,
    ) -> anyhow::Result<()> {
        let extractor = self
            .extractor
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("UDP extractor not set"))?
            .clone();

        let mut buf = [0u8; 65535];
        loop {
            let (n, peer_addr) = socket.recv_from(&mut buf).await?;
            match extractor(&buf[..n]) {
                Ok(Some((key, frame, cmd))) => {
                    if let Some(handler) = self.handlers.get(&key).cloned() {
                        let global = global.clone();
                        let socket = socket.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handler(global, frame, cmd, peer_addr, socket).await {
                                tracing::error!("UDP Handler Error: {:?}", e);
                            }
                        });
                    }
                }
                Ok(None) => {}
                Err(e) => tracing::warn!("dropping invalid datagram: {e}"),
            }
        }
    }
}
