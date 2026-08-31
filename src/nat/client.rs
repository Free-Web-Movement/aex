//! 内网节点隧道客户端（tunnel client）。
//!
//! 处于传输层以下，直接操作 `TcpStream`。内网节点（Edge，NAT 后）主动出站
//! 连公网中继节点，注册自己并学习自己的公网映射地址，之后经中继收发数据。

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, mpsc};

use super::types::{NatError, NatFrame, NatFrameType, NatResult};

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
            ready_rx: Mutex::new(Some(ready_rx)),
            ready_tx: Mutex::new(Some(ready_tx)),
        });

        // 发送注册帧。
        let reg = NatFrame::register(&client.node_id);
        client.write_frame(&reg).await?;

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
        let bytes = frame.encode()?;
        let mut guard = self.writer.lock().await;
        let writer = guard.as_mut().ok_or(NatError::RemoteClosed)?;
        writer
            .write_all(&(bytes.len() as u32).to_le_bytes())
            .await?;
        writer.write_all(&bytes).await?;
        Ok(())
    }

    async fn read_loop(&self) -> NatResult<()> {
        let mut reader = match self.reader.lock().await.take() {
            Some(r) => r,
            None => return Err(NatError::RemoteClosed),
        };
        let mut len_buf = [0u8; LEN_PREFIX_BYTES];
        loop {
            match reader.read_exact(&mut len_buf).await {
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
            let len = u32::from_le_bytes(len_buf) as usize;
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
                    if let Some(tx) = self.data_tx.lock().await.clone() {
                        let _ = tx.send(TunnelData {
                            from: frame.src,
                            payload: frame.payload,
                        });
                    }
                }
                _ => {
                    tracing::debug!("NAT tunnel: received {:?}", frame.frame_type);
                }
            }
        }
    }
}
