use dashmap::DashMap;
use tokio_util::sync::CancellationToken;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::net::tcp::OwnedWriteHalf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NetworkScope {
    Intranet, // 内网 (RFC1918, IPv6 LLA/ULA)
    Extranet, // 外网 (公网 IP)
}

#[derive(Debug, Clone)]
pub struct ConnectionEntry {
    pub addr: SocketAddr,
    pub writer: Arc<tokio::sync::Mutex<OwnedWriteHalf>>,
    pub abort_handle: tokio::task::AbortHandle,
    pub cancel_token: CancellationToken,
    pub connected_at: u64,
    /// 最后活跃时间戳（秒）
    pub last_seen: Arc<AtomicU64>,
}

impl ConnectionEntry {
    pub fn uptime_secs(&self) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now.saturating_sub(self.connected_at)
    }

    /// 判定该连接是否应当被停用（根据心跳限制和寿命限制）
    /// @param timeout_secs: 最大静默允许时间
    /// @param max_lifetime_secs: 最大允许存活时长
    pub fn is_deactivated(&self, current: u64, timeout_secs: u64, max_lifetime_secs: u64) -> bool {
        // 1. 检查寿命
        let uptime = current.saturating_sub(self.connected_at);
        if uptime > max_lifetime_secs {
            return true;
        }

        // 2. 检查活跃度
        let last_active = self.last_seen.load(Ordering::Relaxed);
        let idle_time = current.saturating_sub(last_active);
        if idle_time > timeout_secs {
            return true;
        }

        false
    }
}


impl Drop for ConnectionEntry {
    fn drop(&mut self) {
        // 确保当 Entry 彻底离开内存时，协程任务一定停止
        self.abort_handle.abort();
    }
}

#[derive(Debug)]
pub struct BiDirectionalConnections {
    pub clients: DashMap<SocketAddr, Arc<ConnectionEntry>>,
    /// 出站连接池：我们主动连出的节点 (Outbound)
    pub servers: DashMap<SocketAddr, Arc<ConnectionEntry>>,
}

impl BiDirectionalConnections {
    pub fn new() -> Self {
        Self {
            clients: DashMap::new(),
            servers: DashMap::new(),
        }
    }
}
