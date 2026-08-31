//! TCP 打洞（Simultaneous Open）——两内网节点互为服务器与客户端。
//!
//! 经公网中继交换公网映射地址后，双方**同时**向对方公网映射地址发起 TCP
//! 连接（Simultaneous Open）：每端既 listen 又 connect。各自 NAT 已发出过
//! 出站 SYN，视为会话活跃，放行对端随后到来的入站 SYN，双方回 ACK 完成握手，
//! 建立**双向直连**——中继退出数据路径。

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpSocket};
use tokio::sync::Mutex;

use super::types::{NatError, NatResult};

/// 打洞重试次数。
const PUNCH_RETRIES: usize = 8;

/// 双向直连（互为 server/client 的 TCP 连接）。
pub struct PunchTunnel {
    /// 对端公网映射地址（打洞目标）。
    pub peer_public: SocketAddr,
    /// 本机实际绑定地址（同 NAT 时可能是私网地址）。
    pub local_addr: SocketAddr,
    /// 读端。
    pub reader: tokio::net::tcp::OwnedReadHalf,
    /// 写端。
    pub writer: tokio::net::tcp::OwnedWriteHalf,
}

impl PunchTunnel {
    /// TCP Simultaneous Open：同时 listen + connect，建立双向直连。
    ///
    /// `peer_public`：对端公网映射地址（由中继 `PunchHint` 提供）。
    /// `local_bind`：本机用于打洞的本地地址（`0.0.0.0:0` 自动分配，或同
    /// NAT 下的私网地址）。
    pub async fn connect(
        peer_public: SocketAddr,
        local_bind: SocketAddr,
    ) -> NatResult<PunchTunnel> {
        for attempt in 0..PUNCH_RETRIES {
            let bind_addr = if local_bind.ip().is_unspecified() {
                SocketAddr::new(IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED), 0)
            } else {
                local_bind
            };
            let bind_port = bind_addr.port();

            // 打洞：listen 固定端口接受对端入站（Simultaneous Open），
            // 同时从随机端口 connect 对端公网映射地址（建立 NAT 映射）。
            let listener = TcpListener::bind(SocketAddr::new(bind_addr.ip(), bind_port))
                .await
                .ok();
            let listen_addr = listener
                .as_ref()
                .map(|l| l.local_addr().unwrap_or(bind_addr));

            // accept 任务：接受对端穿过 NAT 的入站连接。
            let accept_slot = Arc::new(Mutex::new(None));
            let connect_slot = Arc::new(Mutex::new(None));
            let accept_handle = if let Some(l) = listener {
                let slot = accept_slot.clone();
                Some(tokio::spawn(async move {
                    if let Ok(Ok((stream, _))) =
                        tokio::time::timeout(Duration::from_secs(3), l.accept()).await
                    {
                        *slot.lock().await = Some(stream);
                    }
                }))
            } else {
                None
            };

            // connect 任务：出站 SYN 打洞。
            let cslot = connect_slot.clone();
            let connect_handle = tokio::spawn(async move {
                if let Ok(socket) = TcpSocket::new_v4() {
                    let _ = socket.bind(SocketAddr::new(
                        IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED),
                        0,
                    ));
                    if let Ok(stream) = socket.connect(peer_public).await {
                        *cslot.lock().await = Some(stream);
                    }
                }
            });

            // 等待 connect 或 accept 任一成功（上限 ~1.5s）。
            let wait = tokio::time::sleep(Duration::from_millis(1200));
            tokio::pin!(wait);
            tokio::select! {
                _ = &mut wait => {}
                _ = async {
                    loop {
                        if accept_slot.lock().await.is_some()
                            || connect_slot.lock().await.is_some()
                        {
                            return;
                        }
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                } => {}
            }

            let mut stream_opt = accept_slot.lock().await.take();
            if stream_opt.is_none() {
                stream_opt = connect_slot.lock().await.take();
            }

            if let Some(handle) = &accept_handle {
                handle.abort();
            }
            connect_handle.abort();

            if let Some(stream) = stream_opt {
                let local_addr = stream.local_addr().unwrap_or(bind_addr);
                let (reader, writer) = stream.into_split();
                tracing::info!(
                    "🔓 TCP punch SUCCESS: local {} <-> peer {} (attempt {})",
                    local_addr,
                    peer_public,
                    attempt
                );
                return Ok(PunchTunnel {
                    peer_public,
                    local_addr,
                    reader,
                    writer,
                });
            }

            let _ = listen_addr;
            // 打洞需同时进行：对端可能尚未发包，稍等重试。
            tokio::time::sleep(Duration::from_millis(150 * (attempt as u64 + 1))).await;
        }
        Err(NatError::Io(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "punch timeout",
        )))
    }

    /// 向对端发送数据。
    pub async fn send(&mut self, data: &[u8]) -> NatResult<()> {
        self.writer
            .write_all(&(data.len() as u32).to_le_bytes())
            .await?;
        self.writer.write_all(data).await?;
        Ok(())
    }

    /// 读取一条数据（长度前缀 + 载荷）。
    pub async fn recv(&mut self) -> NatResult<Vec<u8>> {
        let mut len_buf = [0u8; 4];
        self.reader.read_exact(&mut len_buf).await?;
        let len = u32::from_le_bytes(len_buf) as usize;
        let mut buf = vec![0u8; len];
        self.reader.read_exact(&mut buf).await?;
        Ok(buf)
    }
}

/// 打洞状态（供上层判定直连是否建立）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PunchState {
    Idle,
    Punching,
    Connected,
    Failed,
}

/// 打洞协调器：发起并跟踪一次直连。
#[derive(Clone)]
pub struct PunchCoordinator {
    state: Arc<Mutex<PunchState>>,
}

impl Default for PunchCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

impl PunchCoordinator {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(PunchState::Idle)),
        }
    }

    pub async fn state(&self) -> PunchState {
        *self.state.lock().await
    }

    /// 尝试与对端建立直连（TCP simultaneous open）。
    ///
    /// 返回成功时即建立互为 server/client 的双向连接。
    pub async fn punch(
        &self,
        peer_public: SocketAddr,
        local_bind: SocketAddr,
    ) -> NatResult<PunchTunnel> {
        *self.state.lock().await = PunchState::Punching;
        match PunchTunnel::connect(peer_public, local_bind).await {
            Ok(t) => {
                *self.state.lock().await = PunchState::Connected;
                Ok(t)
            }
            Err(e) => {
                *self.state.lock().await = PunchState::Failed;
                Err(e)
            }
        }
    }
}
