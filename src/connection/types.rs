use crate::connection::node::Node;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::net::tcp::OwnedWriteHalf;
use tokio_util::sync::CancellationToken;
use tokio::sync::{Mutex, RwLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NetworkScope {
    Intranet, // 内网 (RFC1918, IPv6 LLA/ULA)
    Extranet, // 外网 (公网 IP)
}


impl NetworkScope {
    pub fn from_ip(ip: &std::net::IpAddr) -> Self {
        match ip {
            std::net::IpAddr::V4(v4) => {
                if v4.is_loopback() || v4.is_private() || v4.is_link_local() {
                    NetworkScope::Intranet
                } else {
                    NetworkScope::Extranet
                }
            }
            std::net::IpAddr::V6(v6) => {
                if v6.is_loopback() || v6.is_unicast_link_local() || (v6.segments()[0] & 0xfe00) == 0xfc00 {
                    NetworkScope::Intranet
                } else {
                    NetworkScope::Extranet
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConnectionEntry {
    /// 💡 新增：节点的静态信息（ID, Version, 声明的 IPs 等）
    /// 这个数据在握手成功后填入，并在连接生命周期内保持不变
    pub node: Arc<RwLock<Option<Node>>>,
    pub addr: SocketAddr,
    pub writer: Arc<tokio::sync::Mutex<OwnedWriteHalf>>,
    pub abort_handle: tokio::task::AbortHandle,
    pub cancel_token: CancellationToken,
    pub connected_at: u64,
    /// 最后活跃时间戳（秒）
    pub last_seen: Arc<AtomicU64>,
}

impl ConnectionEntry {

    pub fn new_empty_node(addr: SocketAddr, writer: OwnedWriteHalf, handle: tokio::task::AbortHandle, cancel_token: CancellationToken) -> Self {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        Self {
            node: Arc::new(RwLock::new(None)),
            addr,
            writer: Arc::new(Mutex::new(writer)),
            abort_handle: handle,
            cancel_token,
            connected_at: now,
            last_seen: Arc::new(AtomicU64::new(now)),
        }
    }

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

    /// 动态更新节点信息（例如收到对方的地址交换报文或心跳包时）
    pub async fn update_node(&self, new_node: Node) {
        let mut lock = self.node.write().await;
        *lock = Some(new_node);
    }

    /// 尝试获取当前的节点 ID
    pub async fn get_peer_id(&self) -> Option<Vec<u8>> {
        let lock = self.node.read().await;
        lock.as_ref().map(|n| n.id.clone())
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
