use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use async_lock::RwLock;

use crate::constants::udp::DEFAULT_MULTICAST_TTL;

#[derive(Debug, Clone)]
pub struct MulticastGroup {
    pub addr: SocketAddr,
    pub scope: MulticastScope,
    pub members: Arc<RwLock<HashMap<SocketAddr, MulticastMember>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MulticastScope {
    Local,        // 224.0.0.0 - 224.0.0.255
    Admin,        // 224.0.2.0 - 224.0.255.255
    SiteLocal,    // 239.255.0.0 - 239.255.255.255
    Organization, // 239.192.0.0 - 239.192.255.255
    Global,       // 224.0.1.0 - 238.255.255.255
}

impl MulticastScope {
    pub fn from_addr(addr: &Ipv4Addr) -> Option<Self> {
        let octets = addr.octets();
        match octets[0] {
            224..=224 => Some(Self::Local),
            225..=228 => Some(Self::Global),
            239 if octets[1] == 0 => Some(Self::Admin),
            239 if octets[1] == 255 => Some(Self::SiteLocal),
            239 if octets[1] == 192 => Some(Self::Organization),
            _ => None,
        }
    }

    pub fn is_admin_local(&self) -> bool {
        matches!(self, Self::Local | Self::Admin)
    }

    pub fn address_range(&self) -> (Ipv4Addr, Ipv4Addr) {
        match self {
            Self::Local => (Ipv4Addr::new(224, 0, 0, 0), Ipv4Addr::new(224, 0, 0, 255)),
            Self::Admin => (Ipv4Addr::new(224, 0, 2, 0), Ipv4Addr::new(224, 0, 255, 255)),
            Self::SiteLocal => (
                Ipv4Addr::new(239, 255, 0, 0),
                Ipv4Addr::new(239, 255, 255, 255),
            ),
            Self::Organization => (
                Ipv4Addr::new(239, 192, 0, 0),
                Ipv4Addr::new(239, 192, 255, 255),
            ),
            Self::Global => (
                Ipv4Addr::new(224, 0, 1, 0),
                Ipv4Addr::new(238, 255, 255, 255),
            ),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MulticastMember {
    pub addr: SocketAddr,
    pub joined_at: u64,
    pub last_seen: u64,
    pub ttl: u8,
}

impl MulticastGroup {
    pub fn new(addr: SocketAddr) -> Self {
        let scope = if let std::net::IpAddr::V4(ipv4) = addr.ip() {
            MulticastScope::from_addr(&ipv4).unwrap_or(MulticastScope::SiteLocal)
        } else {
            MulticastScope::SiteLocal
        };
        Self {
            addr,
            scope,
            members: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn new_site_local(port: u16) -> Self {
        Self::new(SocketAddr::new(
            Ipv4Addr::new(239, 255, 255, 254).into(),
            port,
        ))
    }

    pub async fn join(&self, peer: SocketAddr) {
        let mut members = self.members.write().await;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        members.insert(
            peer,
            MulticastMember {
                addr: peer,
                joined_at: now,
                last_seen: now,
                ttl: DEFAULT_MULTICAST_TTL,
            },
        );
    }

    pub async fn leave(&self, peer: &SocketAddr) {
        let mut members = self.members.write().await;
        members.remove(peer);
    }

    pub async fn members(&self) -> Vec<SocketAddr> {
        let members = self.members.read().await;
        members.keys().cloned().collect()
    }

    pub async fn member_count(&self) -> usize {
        self.members.read().await.len()
    }

    pub async fn is_member(&self, peer: &SocketAddr) -> bool {
        self.members.read().await.contains_key(peer)
    }
}

pub struct MulticastManager {
    groups: Arc<RwLock<HashMap<SocketAddr, MulticastGroup>>>,
}

impl MulticastManager {
    pub fn new() -> Self {
        Self {
            groups: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn create_group(&self, addr: SocketAddr) -> MulticastGroup {
        let group = MulticastGroup::new(addr);
        self.groups.write().await.insert(addr, group.clone());
        group
    }

    pub async fn get_group(&self, addr: &SocketAddr) -> Option<MulticastGroup> {
        self.groups.read().await.get(addr).cloned()
    }

    pub async fn remove_group(&self, addr: &SocketAddr) {
        self.groups.write().await.remove(addr);
    }

    pub async fn all_groups(&self) -> Vec<SocketAddr> {
        self.groups.read().await.keys().cloned().collect()
    }
}

impl Default for MulticastManager {
    fn default() -> Self {
        Self::new()
    }
}
