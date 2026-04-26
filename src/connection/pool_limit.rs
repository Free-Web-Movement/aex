use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use async_lock::RwLock;

pub struct ConnectionPoolConfig {
    pub max_total_connections: usize,
    pub max_connections_per_ip: usize,
    pub max_connections_per_subnet: usize,
    pub max_outbound_connections: usize,
    pub max_inbound_connections: usize,
    pub cleanup_interval_secs: u64,
    pub idle_timeout_secs: u64,
}

impl ConnectionPoolConfig {
    pub fn new(max_total: usize) -> Self {
        Self {
            max_total_connections: max_total,
            max_connections_per_ip: 10,
            max_connections_per_subnet: 100,
            max_outbound_connections: max_total / 2,
            max_inbound_connections: max_total / 2,
            cleanup_interval_secs: 60,
            idle_timeout_secs: 300,
        }
    }

    pub fn with_per_ip_limit(mut self, limit: usize) -> Self {
        self.max_connections_per_ip = limit;
        self
    }

    pub fn with_subnet_limit(mut self, limit: usize) -> Self {
        self.max_connections_per_subnet = limit;
        self
    }

    pub fn with_idle_timeout(mut self, secs: u64) -> Self {
        self.idle_timeout_secs = secs;
        self
    }
}

impl Default for ConnectionPoolConfig {
    fn default() -> Self {
        Self::new(1000)
    }
}

pub struct ConnectionPoolLimits {
    config: ConnectionPoolConfig,
    active_connections: Arc<RwLock<HashMap<SocketAddr, ConnectionInfo>>>,
    connection_counts: Arc<RwLock<HashMap<SocketAddr, u32>>>,
    subnet_counts: Arc<RwLock<HashMap<String, u32>>>,
    outbound_count: Arc<RwLock<u32>>,
    inbound_count: Arc<RwLock<u32>>,
}

#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    pub addr: SocketAddr,
    pub is_outbound: bool,
    pub created_at: u64,
    pub last_active: u64,
}

impl ConnectionPoolLimits {
    pub fn new(config: ConnectionPoolConfig) -> Self {
        Self {
            config,
            active_connections: Arc::new(RwLock::new(HashMap::new())),
            connection_counts: Arc::new(RwLock::new(HashMap::new())),
            subnet_counts: Arc::new(RwLock::new(HashMap::new())),
            outbound_count: Arc::new(RwLock::new(0)),
            inbound_count: Arc::new(RwLock::new(0)),
        }
    }

    fn get_subnet(addr: &SocketAddr) -> String {
        let ip = addr.ip();
        if let std::net::IpAddr::V4(ipv4) = ip {
            let octets = ipv4.octets();
            format!("{}.{}.{}.0/24", octets[0], octets[1], octets[2])
        } else {
            format!("ipv6_global")
        }
    }

    pub async fn can_connect(&self, addr: &SocketAddr, is_outbound: bool) -> PoolAllowResult {
        let counts = self.connection_counts.read().await;
        let total = counts.values().sum::<u32>() as usize;
        if total >= self.config.max_total_connections {
            return PoolAllowResult::TotalLimit;
        }

        let per_ip = *counts.get(addr).unwrap_or(&0) as usize;
        if per_ip >= self.config.max_connections_per_ip {
            return PoolAllowResult::PerIpLimit;
        }

        let subnet = Self::get_subnet(addr);
        let subnet_counts = self.subnet_counts.read().await;
        let per_subnet = *subnet_counts.get(&subnet).unwrap_or(&0) as usize;
        if per_subnet >= self.config.max_connections_per_subnet {
            return PoolAllowResult::SubnetLimit;
        }

        if is_outbound {
            let outbound = *self.outbound_count.read().await as usize;
            if outbound >= self.config.max_outbound_connections {
                return PoolAllowResult::OutboundLimit;
            }
        } else {
            let inbound = *self.inbound_count.read().await as usize;
            if inbound >= self.config.max_inbound_connections {
                return PoolAllowResult::InboundLimit;
            }
        }

        PoolAllowResult::Allowed
    }

    pub async fn add_connection(&self, addr: SocketAddr, is_outbound: bool) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let info = ConnectionInfo {
            addr,
            is_outbound,
            created_at: now,
            last_active: now,
        };
        self.active_connections.write().await.insert(addr, info);

        *self
            .connection_counts
            .write()
            .await
            .entry(addr)
            .or_insert(0) += 1;

        let subnet = Self::get_subnet(&addr);
        *self.subnet_counts.write().await.entry(subnet).or_insert(0) += 1;

        if is_outbound {
            *self.outbound_count.write().await += 1;
        } else {
            *self.inbound_count.write().await += 1;
        }
    }

    pub async fn remove_connection(&self, addr: &SocketAddr) {
        let removed = self.active_connections.write().await.remove(addr);
        if removed.is_none() {
            return;
        }

        if let Some(count) = self.connection_counts.write().await.get_mut(addr) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                self.connection_counts.write().await.remove(addr);
            }
        }

        let subnet = Self::get_subnet(addr);
        if let Some(count) = self.subnet_counts.write().await.get_mut(&subnet) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                self.subnet_counts.write().await.remove(&subnet);
            }
        }

        if let Some(info) = removed {
            if info.is_outbound {
                *self.outbound_count.write().await =
                    self.outbound_count.read().await.saturating_sub(1);
            } else {
                *self.inbound_count.write().await =
                    self.inbound_count.read().await.saturating_sub(1);
            }
        }
    }

    pub async fn total_connections(&self) -> usize {
        self.connection_counts.read().await.values().sum::<u32>() as usize
    }

    pub async fn per_ip_count(&self, addr: &SocketAddr) -> usize {
        *self.connection_counts.read().await.get(addr).unwrap_or(&0) as usize
    }

    pub async fn outbound_count(&self) -> usize {
        *self.outbound_count.read().await as usize
    }

    pub async fn inbound_count(&self) -> usize {
        *self.inbound_count.read().await as usize
    }

    pub async fn cleanup_idle(&self) -> Vec<SocketAddr> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut to_remove = Vec::new();
        let mut connections = self.active_connections.write().await;

        for (addr, info) in connections.iter() {
            if now - info.last_active > self.config.idle_timeout_secs {
                to_remove.push(*addr);
            }
        }

        for addr in &to_remove {
            connections.remove(addr);
        }

        to_remove
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolAllowResult {
    Allowed,
    TotalLimit,
    PerIpLimit,
    SubnetLimit,
    OutboundLimit,
    InboundLimit,
}

impl PoolAllowResult {
    pub fn is_allowed(&self) -> bool {
        matches!(self, PoolAllowResult::Allowed)
    }
}
