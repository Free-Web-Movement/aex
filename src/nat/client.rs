//! 内网节点隧道客户端（tunnel client）。
//!
//! 处于传输层以下，直接操作 `TcpStream`。内网节点（Edge，NAT 后）主动出站
//! 连公网中继节点，注册自己并学习自己的公网映射地址，之后经中继收发数据。
//! 支持打洞：经中继交换对端公网映射地址后，同时向对端 TCP 打洞，建立互为
//! server/client 的直连（中继退出数据路径）；打洞失败回退中继转发。

use std::collections::HashMap;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll};
use std::time::Duration;

use dashmap::DashMap;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, mpsc};

use super::punch::{PunchCoordinator, PunchTunnel};
use super::types::{
    frame_len_from_header, NatError, NatFrame, NatFrameType, NatResult, NAT_MAGIC_LEN,
};

const LEN_PREFIX_BYTES: usize = 4;

/// 隧道连接状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TunnelState {
    /// 已连接公网节点，注册确认前。
    Connecting,
    /// 已注册，隧道可用。
    Ready,
    /// 已断开。
    Disconnected,
}

/// 收到的数据通知（供上层消费）。
#[derive(Debug, Clone)]
pub struct TunnelData {
    /// 发送方节点身份。
    pub from: String,
    /// 载荷。
    pub payload: Vec<u8>,
}

/// 打洞直连建立通知（互为 server/client 的双向连接）。
#[derive(Clone)]
pub struct PunchEstablished {
    /// 对端节点身份。
    pub peer: String,
    /// 对端公网映射地址。
    pub peer_public: SocketAddr,
    /// 建立的直连隧道。
    pub tunnel: Arc<PunchTunnel>,
}

/// 与某对端的已建立连接（流对象）通知。
///
/// aex 自动发现 peer 并建立连接后，把流对象交给上层（fwmc）使用：
/// 上层用 `AsyncRead`/`AsyncWrite` 收发 P2P 消息，无需感知 NAT。
pub struct PeerStreamEstablished {
    /// 对端节点身份。
    pub peer: String,
    /// 到对端的双向字节流（写=发给该 peer，读=该 peer 发来的数据）。
    pub stream: NatTunnelChannel,
}

/// 内网节点隧道客户端。
pub struct NatTunnelClient {
    /// 本节点身份。
    pub node_id: String,
    /// 公网中继节点地址。
    pub relay_addr: SocketAddr,
    /// 注册确认后获得的公网映射地址（`ip:port`）。
    public_addr: Mutex<Option<String>>,
    /// 读端（用于后台读循环）。
    reader: Mutex<Option<tokio::net::tcp::OwnedReadHalf>>,
    /// 写端。
    writer: Mutex<Option<tokio::net::tcp::OwnedWriteHalf>>,
    /// 当前状态。
    state: Mutex<TunnelState>,
    /// 数据接收通道（收到 Data 帧转发给上层）。
    data_tx: Mutex<Option<mpsc::UnboundedSender<TunnelData>>>,
    /// 打洞直连建立通道。
    punch_tx: Mutex<Option<mpsc::UnboundedSender<PunchEstablished>>>,
    /// 已建立 peer 连接（流对象）通知通道。
    stream_tx: Mutex<Option<mpsc::UnboundedSender<PeerStreamEstablished>>>,
    /// 已收集的对端公网映射地址：node_id → SocketAddr。
    punch_peers: Mutex<HashMap<String, SocketAddr>>,
    /// 打洞时本地绑定的端口（默认随机）。
    ///
    /// 公网节点（无 NAT）自我注册后，其 PunchHint 广播的公网映射端口 = 本端口。
    /// 打洞时从该端口 listen + connect，使对端向该端口出站能命中本节点 listener，
    /// 实现「出站命中对端 listen」的 simultaneous open 直连。
    punch_local_port: Mutex<Option<u16>>,
    /// 按对端路由的数据通道：peer_id → 该对端 DATA 载荷的接收端。
    ///
    /// 上层经 [`NatTunnelClient::open_channel`] 打开某个对端的双向字节流，
    /// 收到该对端的 DATA 帧后按 `from` 路由到对应通道（隧道低层能力）。
    channels: DashMap<String, mpsc::UnboundedSender<Vec<u8>>>,
    /// 自身 Arc 引用（`open_channel` 需要 `Arc<Self>`）。
    self_ref: Mutex<Option<Arc<Self>>>,
    /// 注册确认等待。
    ready_rx: Mutex<Option<tokio::sync::oneshot::Receiver<bool>>>,
    ready_tx: Mutex<Option<tokio::sync::oneshot::Sender<bool>>>,
}

impl NatTunnelClient {
    /// 连接公网中继节点并注册。
    pub async fn connect(node_id: String, relay_addr: SocketAddr) -> NatResult<Arc<Self>> {
        let stream = TcpStream::connect(relay_addr).await?;
        let (reader, writer) = stream.into_split();
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let client = Arc::new(Self {
            node_id,
            relay_addr,
            public_addr: Mutex::new(None),
            reader: Mutex::new(Some(reader)),
            writer: Mutex::new(Some(writer)),
            state: Mutex::new(TunnelState::Connecting),
            data_tx: Mutex::new(None),
            punch_tx: Mutex::new(None),
            punch_peers: Mutex::new(HashMap::new()),
            punch_local_port: Mutex::new(None),
            stream_tx: Mutex::new(None),
            channels: DashMap::new(),
            self_ref: Mutex::new(None),
            ready_rx: Mutex::new(Some(ready_rx)),
            ready_tx: Mutex::new(Some(ready_tx)),
        });

        // 发送注册帧。
        let reg = NatFrame::register(&client.node_id);
        client.write_frame(&reg).await?;

        // 记录自身 Arc，供 open_channel 使用。
        *client.self_ref.lock().await = Some(client.clone());

        // 启动后台读循环（处理 RegisterAck / KeepAliveAck / Data）。
        let reader_client = client.clone();
        tokio::spawn(async move {
            let _ = reader_client.read_loop().await;
        });

        Ok(client)
    }

    /// 等待注册确认（默认 5s 超时）。
    pub async fn wait_ready(&self, timeout: Duration) -> NatResult<()> {
        let rx = self
            .ready_rx
            .lock()
            .await
            .take()
            .ok_or(NatError::Protocol("wait_ready called twice".into()))?;
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(_)) => Err(NatError::RemoteClosed),
            Err(_) => Err(NatError::Protocol("register timeout".into())),
        }
    }

    /// 设置数据接收通道。
    pub async fn set_data_channel(&self, tx: mpsc::UnboundedSender<TunnelData>) {
        *self.data_tx.lock().await = Some(tx);
    }

    /// 设置打洞直连建立通知通道。
    pub async fn set_punch_channel(&self, tx: mpsc::UnboundedSender<PunchEstablished>) {
        *self.punch_tx.lock().await = Some(tx);
    }

    /// 设置已建立 peer 连接（流对象）通知通道。
    ///
    /// aex 自动发现并建立到对端的连接后，通过此通道把流对象交给上层。
    pub async fn set_stream_channel(&self, tx: mpsc::UnboundedSender<PeerStreamEstablished>) {
        *self.stream_tx.lock().await = Some(tx);
    }

    /// 设置打洞时本地绑定端口。
    ///
    /// 无 NAT 的公网节点自我注册后应设为自身 PunchHint 广播的映射端口，
    /// 使对端向该端口出站打洞时能命中本节点 listener。
    pub async fn set_punch_local_port(&self, port: Option<u16>) {
        *self.punch_local_port.lock().await = port;
    }

    /// 打开与某个对端的双向字节通道。
    ///
    /// 返回 [`NatTunnelChannel`]：实现 `AsyncRead`（读该对端经中继发来的 DATA
    /// 载荷）+ `AsyncWrite`（写 → `send_to` 发给该对端）。隧道低层能力，与上层
    /// 协议无关，供把 NAT 隧道接入任意字节流协议使用。
    pub async fn open_channel(&self, peer: &str) -> NatTunnelChannel {
        let (tx, rx) = mpsc::unbounded_channel();
        self.channels.insert(peer.to_string(), tx);
        NatTunnelChannel {
            client: self
                .self_ref
                .lock()
                .await
                .clone()
                .unwrap_or_else(|| unreachable!("self_ref must be set after connect")),
            peer: peer.to_string(),
            pending: Vec::new(),
            rx,
        }
    }

    /// 发起与对端节点的打洞（经中继交换公网映射地址后 TCP 同时打洞）。
    ///
    /// 返回后对端公网映射地址尚未交换完成；打洞结果通过 `set_punch_channel`
    /// 通知（[`PunchEstablished`]）。打洞失败时数据仍经中继转发（降级）。
    pub async fn request_punch(&self, dst: &str) -> NatResult<()> {
        let frame = NatFrame::punch_request(&self.node_id, dst);
        self.write_frame(&frame).await
    }

    /// 查询已收集的对端公网映射地址。
    pub async fn peer_public_addr(&self, peer: &str) -> Option<SocketAddr> {
        self.punch_peers.lock().await.get(peer).copied()
    }

    /// 当前公网映射地址（注册确认后非空）。
    pub async fn public_addr(&self) -> Option<String> {
        self.public_addr.lock().await.clone()
    }

    /// 当前隧道状态。
    pub async fn state(&self) -> TunnelState {
        *self.state.lock().await
    }

    /// 发送数据给目标节点（经中继转发）。
    pub async fn send_to(&self, dst: &str, payload: Vec<u8>) -> NatResult<()> {
        let frame = NatFrame::data(&self.node_id, dst, payload);
        self.write_frame(&frame).await
    }

    /// 主动发送保活。
    pub async fn send_keepalive(&self) -> NatResult<()> {
        let frame = NatFrame::keep_alive(&self.node_id);
        self.write_frame(&frame).await
    }

    /// 关闭隧道。
    pub async fn shutdown(&self) {
        *self.state.lock().await = TunnelState::Disconnected;
        *self.writer.lock().await = None;
    }

    async fn write_frame(&self, frame: &NatFrame) -> NatResult<()> {
        let bytes = frame.encode_wire()?;
        let mut guard = self.writer.lock().await;
        let writer = guard.as_mut().ok_or(NatError::RemoteClosed)?;
        writer.write_all(&bytes).await?;
        Ok(())
    }

    async fn read_loop(&self) -> NatResult<()> {
        let mut reader = match self.reader.lock().await.take() {
            Some(r) => r,
            None => return Err(NatError::RemoteClosed),
        };
        let mut head_buf = [0u8; NAT_MAGIC_LEN + LEN_PREFIX_BYTES];
        loop {
            // 帧头：魔数 + 长度。
            match reader.read_exact(&mut head_buf).await {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    *self.state.lock().await = TunnelState::Disconnected;
                    if let Some(tx) = self.ready_tx.lock().await.take() {
                        let _ = tx.send(false);
                    }
                    return Err(NatError::RemoteClosed);
                }
                Err(e) => return Err(e.into()),
            }
            let len = frame_len_from_header(&head_buf)?;
            let mut frame_buf = vec![0u8; len];
            reader.read_exact(&mut frame_buf).await?;
            let frame = NatFrame::decode(&frame_buf)?;
            match frame.frame_type {
                NatFrameType::RegisterAck => {
                    *self.public_addr.lock().await = Some(frame.public_addr.clone());
                    *self.state.lock().await = TunnelState::Ready;
                    tracing::info!(
                        "🧭 NAT tunnel {} ready, public addr = {}",
                        self.node_id,
                        frame.public_addr
                    );
                    if let Some(tx) = self.ready_tx.lock().await.take() {
                        let _ = tx.send(true);
                    }
                }
                NatFrameType::Data => {
                    // 优先路由到按对端打开的字节通道；否则走传统 data_channel。
                    if let Some(tx) = self.channels.get(&frame.src) {
                        let _ = tx.send(frame.payload);
                    } else if let Some(tx) = self.data_tx.lock().await.clone() {
                        let _ = tx.send(TunnelData {
                            from: frame.src,
                            payload: frame.payload,
                        });
                    }
                }
                NatFrameType::PunchHint => {
                    // 中继告知对端公网映射地址（extra = 对端 `ip:port`）。
                    if let Ok(peer_addr) = frame.extra.parse::<SocketAddr>() {
                        self.punch_peers
                            .lock()
                            .await
                            .insert(frame.src.clone(), peer_addr);
                        tracing::info!(
                            "🔓 NAT punch: peer {} public addr = {}",
                            frame.src,
                            peer_addr
                        );
                    }
                }
                NatFrameType::PunchStart => {
                    // 通知开始打洞：对端公网地址（由 PunchHint 提供）。
                    // 对端 = 帧里「不是自己」的一方：
                    //   - 目标方收到 src=发起方 → 打洞对端 = src
                    //   - 发起方收到 src=自己(dst=目标) → 打洞对端 = dst
                    let peer = if frame.src == self.node_id {
                        frame.dst.clone()
                    } else {
                        frame.src.clone()
                    };
                    self.try_punch(&peer).await;
                }
                NatFrameType::Peers => {
                    // 中继广播在线 peer 列表：自动为每个对端建立连接并交付流对象。
                    // 仅当上层注册了流对象消费者（set_stream_channel）时才自动建连，
                    // 否则保持传统 data_channel 行为。
                    if self.stream_tx.lock().await.is_none() {
                        continue;
                    }
                    if let Ok(peers) = bincode::decode_from_slice::<Vec<(String, String)>, _>(
                        &frame.payload,
                        bincode::config::standard(),
                    ) {
                        let (list, _) = peers;
                        for (peer_id, _public) in list {
                            if peer_id.is_empty() || peer_id == self.node_id {
                                continue;
                            }
                            // 已建立则跳过。
                            if self.channels.contains_key(&peer_id) {
                                continue;
                            }
                            let stream = self.open_channel(&peer_id).await;
                            tracing::info!(
                                "🔗 NAT auto-connect: established stream to peer {}",
                                peer_id
                            );
                            if let Some(tx) = self.stream_tx.lock().await.clone() {
                                let _ = tx.send(PeerStreamEstablished {
                                    peer: peer_id,
                                    stream,
                                });
                            }
                        }
                    }
                }
                _ => {
                    tracing::debug!("NAT tunnel: received {:?}", frame.frame_type);
                }
            }
        }
    }

    /// 尝试与对端打洞建立直连（互为 server/client）。
    async fn try_punch(&self, peer: &str) {
        let peer_addr = match self.punch_peers.lock().await.get(peer).copied() {
            Some(a) => a,
            None => {
                tracing::debug!("NAT punch: no peer addr for {}, skip", peer);
                return;
            }
        };
        let local_port = *self.punch_local_port.lock().await;
        let local_bind: SocketAddr = match local_port {
            Some(port) => SocketAddr::new(
                std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED),
                port,
            ),
            None => "0.0.0.0:0".parse().unwrap(),
        };
        let coordinator = PunchCoordinator::new();
        match coordinator.punch(peer_addr, local_bind).await {
            Ok(tunnel) => {
                tracing::info!(
                    "🔓 NAT punch SUCCESS: {} <-> {} (direct P2P)",
                    self.node_id,
                    peer
                );
                if let Some(tx) = self.punch_tx.lock().await.clone() {
                    let _ = tx.send(PunchEstablished {
                        peer: peer.to_string(),
                        peer_public: peer_addr,
                        tunnel: Arc::new(tunnel),
                    });
                }
            }
            Err(e) => {
                // 打洞失败：降级，数据继续经中继转发。
                tracing::info!(
                    "🔓 NAT punch {} failed: {:?} (fallback to relay)",
                    peer,
                    e
                );
            }
        }
    }
}

/// 与某个对端的双向字节通道。
///
/// 实现 `AsyncRead` + `AsyncWrite`，把 NAT 隧道对端抽象成普通字节流：
/// - 写：`poll_write` → 经 [`NatTunnelClient::send_to`] 发 DATA 帧给该对端。
/// - 读：`poll_read` → 从该对端经中继收到的 DATA 载荷（`open_channel` 时
///   注册的通道）。
///
/// 供上层把 NAT 隧道接入任意字节流协议（与具体业务无关）。
pub struct NatTunnelChannel {
    client: Arc<NatTunnelClient>,
    peer: String,
    pending: Vec<u8>,
    rx: mpsc::UnboundedReceiver<Vec<u8>>,
}

impl AsyncRead for NatTunnelChannel {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if self.pending.is_empty() {
            match self.rx.poll_recv(cx) {
                Poll::Ready(Some(data)) => self.pending = data,
                Poll::Ready(None) => {
                    return Poll::Ready(Ok(())); // 对端关闭
                }
                Poll::Pending => return Poll::Pending,
            }
        }
        let n = std::cmp::min(self.pending.len(), buf.remaining());
        buf.put_slice(&self.pending[..n]);
        self.pending.drain(..n);
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for NatTunnelChannel {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut TaskContext<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let data = buf.to_vec();
        let client = self.client.clone();
        let peer = self.peer.clone();
        tokio::spawn(async move {
            let _ = client.send_to(&peer, data).await;
        });
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}
