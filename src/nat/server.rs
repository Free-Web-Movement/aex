//! 公网中继节点（relay / tunnel server）。
//!
//! 处于传输层以下，直接操作 `TcpStream`。内网节点（Edge）主动出站连入本
//! 节点建立隧道；本节点登记每个对端（node_id → 连接），把发往某对端的数据
//! 沿其对端连接反写转发，实现 NAT 后的内网节点互联。
//!
//! 两种运行形态：
//! - [`NatRelayServer::bind`]：独立监听端口（供测试/独立部署）。
//! - [`NatRelayService::handle_conn`]：作为 unified server 的 TCPHandler，
//!   与 HTTP/P2P 共用同一端口（单端口原则）。

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::connection::context::{BoxReader, BoxWriter};

use super::types::{
    NatError, NatFrame, NatFrameType, NatResult, TunnelPeer, NAT_KEEPALIVE_TIMEOUT,
};

/// 帧长度前缀占位大小（u32 LE）。
pub const LEN_PREFIX_BYTES: usize = 4;

/// 公网中继节点（独立 listener 形态）。
pub struct NatRelayServer {
    /// 本机监听地址。
    pub addr: SocketAddr,
    /// 底层 listener。
    listener: TcpListener,
    /// 共享中继服务。
    service: NatRelayService,
    /// 保活清理任务句柄。
    _reaper: tokio::task::AbortHandle,
}

/// 共享中继服务：登记表 + 连接处理（unified 模式下使用）。
#[derive(Clone)]
pub struct NatRelayService {
    /// 登记表：node_id → 对端连接写端 + 公网映射地址。
    peers: Arc<DashMap<String, Arc<PeerConn>>>,
}

struct PeerConn {
    pub addr: SocketAddr,
    pub public_addr: String,
    pub writer: Arc<tokio::sync::Mutex<BoxWriter>>,
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

impl NatRelayService {
    /// 新建共享中继服务（登记表 + reaper）。
    pub fn new() -> Self {
        let peers: Arc<DashMap<String, Arc<PeerConn>>> = Arc::new(DashMap::new());
        let reaper_peers = peers.clone();
        tokio::spawn(async move {
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
        Self { peers }
    }

    /// 处理一个隧道连接（从 socket 提取读写端）。
    pub async fn handle_socket(&self, socket: TcpStream, addr: SocketAddr) -> NatResult<()> {
        let (reader, writer) = socket.into_split();
        let boxed_reader: BoxReader = Box::new(tokio::io::BufReader::new(reader));
        let boxed_writer: BoxWriter = Box::new(writer);
        self.handle_split(boxed_reader, boxed_writer, addr).await
    }

    /// 处理一个隧道连接（unified server 传入的 trait-object 读写端）。
    pub async fn handle_split(
        &self,
        mut reader: BoxReader,
        writer: BoxWriter,
        addr: SocketAddr,
    ) -> NatResult<()> {
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
                    if let Some(prev) = self.peers.get(&frame.src) {
                        if prev.addr != addr {
                            tracing::warn!(
                                "NAT relay: duplicate register for {}, replacing old",
                                frame.src
                            );
                        }
                    }
                    self.peers.insert(frame.src.clone(), conn.clone());
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
                NatFrameType::PunchRequest => {
                    // 打洞信令：交换双方公网映射地址（PunchHint 双向）+ 通知
                    // 双方同时开始打洞（PunchStart 双向）。
                    let src_public = addr.to_string();
                    if let Some(target) = self.peers.get(&frame.dst) {
                        // 目标 B 收到：A 的公网地址 + 开始打洞。
                        let hint_to_target = NatFrame::punch_hint(&frame.src, &src_public);
                        write_frame(&target.writer, &hint_to_target).await?;
                        let start_target = NatFrame::punch_start(&frame.src, &frame.dst);
                        write_frame(&target.writer, &start_target).await?;
                        // 请求者 A 收到：B 的公网地址 + 开始打洞。
                        let hint_to_src = NatFrame::punch_hint(&frame.dst, &target.public_addr);
                        write_frame(&conn.writer, &hint_to_src).await?;
                        let start_src = NatFrame::punch_start(&frame.src, &frame.dst);
                        write_frame(&conn.writer, &start_src).await?;
                    } else {
                        tracing::warn!(
                            "NAT relay: punch target {} not registered",
                            frame.dst
                        );
                    }
                }
                NatFrameType::Data => {
                    relay_data(&self.peers, &conn, &frame).await?;
                }
                _ => {
                    tracing::debug!("NAT relay: ignoring frame {:?}", frame.frame_type);
                }
            }
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

impl Default for NatRelayService {
    fn default() -> Self {
        Self::new()
    }
}

impl NatRelayServer {
    /// 绑定监听地址（`addr:0` 自动分配端口，可通过 `addr` 读取）。
    pub async fn bind(addr: SocketAddr) -> NatResult<Self> {
        let listener = TcpListener::bind(addr).await?;
        let local_addr = listener.local_addr()?;
        Ok(Self {
            addr: local_addr,
            listener,
            service: NatRelayService::new(),
            _reaper: tokio::spawn(async {}).abort_handle(),
        })
    }

    /// 启动 accept 循环，为每个连接派生读任务。
    pub async fn run(&self) -> NatResult<()> {
        tracing::info!("🧭 NAT relay server listening on {}", self.addr);
        loop {
            let (socket, addr) = self.listener.accept().await?;
            let service = self.service.clone();
            tokio::spawn(async move {
                if let Err(e) = service.handle_socket(socket, addr).await {
                    tracing::debug!("NAT relay conn {} ended: {:?}", addr, e);
                }
            });
        }
    }

    /// 当前登记的隧道对端数。
    pub fn peer_count(&self) -> usize {
        self.service.peer_count()
    }

    /// 查询某对端的公网映射地址。
    pub fn peer_public_addr(&self, node_id: &str) -> Option<String> {
        self.service.peer_public_addr(node_id)
    }

    /// 查询登记表（供测试/诊断）。
    pub fn peers_snapshot(&self) -> Vec<TunnelPeer> {
        self.service.peers_snapshot()
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
    writer: &Arc<tokio::sync::Mutex<BoxWriter>>,
    frame: &NatFrame,
) -> NatResult<()> {
    let bytes = frame.encode()?;
    let mut w = writer.lock().await;
    w.write_all(&(bytes.len() as u32).to_le_bytes()).await?;
    w.write_all(&bytes).await?;
    Ok(())
}
