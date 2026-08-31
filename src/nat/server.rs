//! 公网中继节点（relay / tunnel server）。
//!
//! 处于传输层以下，直接操作 `TcpListener`/`TcpStream`。内网节点（Edge）主动
//! 出站连入本节点建立隧道；本节点登记每个对端（node_id → 连接），把发往某
//! 对端的数据沿其对端连接反写转发，实现 NAT 后的内网节点互联。

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use super::types::{
    NatError, NatFrame, NatFrameType, NatResult, TunnelPeer, NAT_KEEPALIVE_TIMEOUT,
};

/// 帧长度前缀占位大小（u32 LE）。
const LEN_PREFIX_BYTES: usize = 4;

/// 公网中继节点。
pub struct NatRelayServer {
    /// 本机监听地址。
    pub addr: SocketAddr,
    /// 底层 listener。
    listener: TcpListener,
    /// 登记表：node_id → 对端连接写端 + 公网映射地址。
    peers: Arc<DashMap<String, Arc<PeerConn>>>,
    /// 保活清理任务句柄。
    _reaper: tokio::task::AbortHandle,
}

struct PeerConn {
    pub addr: SocketAddr,
    pub public_addr: String,
    pub writer: Arc<tokio::sync::Mutex<tokio::net::tcp::OwnedWriteHalf>>,
    pub last_seen: std::sync::atomic::AtomicU64,
}

impl PeerConn {
    fn touch(&self) {
        self.last_seen
            .store(now_secs(), std::sync::atomic::Ordering::Relaxed);
    }

    fn last_seen(&self) -> u64 {
        self.last_seen.load(std::sync::atomic::Ordering::Relaxed)
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl NatRelayServer {
    /// 绑定监听地址（`addr:0` 自动分配端口，可通过 `addr` 读取）。
    pub async fn bind(addr: SocketAddr) -> NatResult<Self> {
        let listener = TcpListener::bind(addr).await?;
        let local_addr = listener.local_addr()?;
        let peers: Arc<DashMap<String, Arc<PeerConn>>> = Arc::new(DashMap::new());
        let reaper_peers = peers.clone();
        let reaper = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            loop {
                interval.tick().await;
                let now = now_secs();
                let stale: Vec<String> = reaper_peers
                    .iter()
                    .filter(|e| {
                        let peer: &PeerConn = &**e.value();
                        now.saturating_sub(peer.last_seen()) >= NAT_KEEPALIVE_TIMEOUT.as_secs()
                    })
                    .map(|e| e.key().clone())
                    .collect();
                for id in stale {
                    if let Some(entry) = reaper_peers.get(&id) {
                        let peer: &PeerConn = &**entry.value();
                        tracing::info!(
                            "🧭 NAT relay: reaping stale peer {} (public {})",
                            peer.addr,
                            peer.public_addr
                        );
                    }
                    reaper_peers.remove(&id);
                }
            }
        });
        Ok(Self {
            addr: local_addr,
            listener,
            peers,
            _reaper: reaper.abort_handle(),
        })
    }

    /// 启动 accept 循环，为每个连接派生读任务。
    pub async fn run(&self) -> NatResult<()> {
        tracing::info!("🧭 NAT relay server listening on {}", self.addr);
        loop {
            let (socket, addr) = self.listener.accept().await?;
            let peers = self.peers.clone();
            tokio::spawn(async move {
                if let Err(e) = handle_conn(peers, socket, addr).await {
                    tracing::debug!("NAT relay conn {} ended: {:?}", addr, e);
                }
            });
        }
    }

    /// 当前登记的隧道对端数。
    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    /// 查询某对端的公网映射地址。
    pub fn peer_public_addr(&self, node_id: &str) -> Option<String> {
        self.peers.get(node_id).map(|p| p.public_addr.clone())
    }

    /// 查询登记表（供测试/诊断）。
    pub fn peers_snapshot(&self) -> Vec<TunnelPeer> {
        self.peers
            .iter()
            .map(|e| TunnelPeer::new(e.key().clone(), e.value().public_addr.clone()))
            .collect()
    }
}

/// 处理一个内网节点的隧道连接。
async fn handle_conn(
    peers: Arc<DashMap<String, Arc<PeerConn>>>,
    socket: TcpStream,
    addr: SocketAddr,
) -> NatResult<()> {
    let (mut reader, writer) = socket.into_split();
    let conn = Arc::new(PeerConn {
        addr,
        public_addr: addr.to_string(),
        writer: Arc::new(tokio::sync::Mutex::new(writer)),
        last_seen: std::sync::atomic::AtomicU64::new(now_secs()),
    });

    let mut len_buf = [0u8; LEN_PREFIX_BYTES];
    loop {
        match reader.read_exact(&mut len_buf).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Err(NatError::RemoteClosed);
            }
            Err(e) => return Err(e.into()),
        }
        let len = u32::from_le_bytes(len_buf) as usize;
        let mut frame_buf = vec![0u8; len];
        reader.read_exact(&mut frame_buf).await?;
        let frame = NatFrame::decode(&frame_buf)?;
        conn.touch();

        match frame.frame_type {
            NatFrameType::Register => {
                if let Some(prev) = peers.get(&frame.src) {
                    if prev.addr != addr {
                        tracing::warn!(
                            "NAT relay: duplicate register for {}, replacing old",
                            frame.src
                        );
                    }
                }
                peers.insert(frame.src.clone(), conn.clone());
                tracing::info!(
                    "🧭 NAT relay: registered {} from {} (public {})",
                    frame.src,
                    addr,
                    addr
                );
                // 回 RegisterAck，携带公网映射地址（内网节点据此知道自己公网地址）。
                let ack = NatFrame::register_ack(&addr.to_string());
                write_frame(&conn.writer, &ack).await?;
            }
            NatFrameType::KeepAlive => {
                let ack = NatFrame::keep_alive_ack(&frame.src);
                write_frame(&conn.writer, &ack).await?;
            }
            NatFrameType::Data => {
                relay_data(&peers, &conn, &frame).await?;
            }
            _ => {
                tracing::debug!("NAT relay: ignoring frame {:?}", frame.frame_type);
            }
        }
    }
}

/// 把数据帧转发给目标节点。
async fn relay_data(
    peers: &Arc<DashMap<String, Arc<PeerConn>>>,
    from: &PeerConn,
    frame: &NatFrame,
) -> NatResult<()> {
    if frame.dst.is_empty() {
        return Ok(());
    }
    let target = peers
        .get(&frame.dst)
        .ok_or_else(|| NatError::PeerNotFound(frame.dst.clone()))?;
    if target.addr == from.addr {
        return Ok(());
    }
    tracing::debug!(
        "🧭 NAT relay: {} -> {} ({} bytes)",
        frame.src,
        frame.dst,
        frame.payload.len()
    );
    write_frame(&target.writer, frame).await?;
    Ok(())
}

async fn write_frame(
    writer: &Arc<tokio::sync::Mutex<tokio::net::tcp::OwnedWriteHalf>>,
    frame: &NatFrame,
) -> NatResult<()> {
    let bytes = frame.encode()?;
    let mut w = writer.lock().await;
    w.write_all(&(bytes.len() as u32).to_le_bytes()).await?;
    w.write_all(&bytes).await?;
    Ok(())
}
