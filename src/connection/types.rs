use std::{net::SocketAddr, sync::Arc};

use dashmap::DashMap;

use crate::connection::entry::ConnectionEntry;

#[derive(Debug, Clone)]
pub struct BiDirectionalConnections {
    pub clients: DashMap<SocketAddr, Arc<ConnectionEntry>>,
    /// 出站连接池：我们主动连出的节点 (Outbound)
    pub servers: DashMap<SocketAddr, Arc<ConnectionEntry>>,
}

impl Default for BiDirectionalConnections {
    fn default() -> Self {
        Self::new()
    }
}

impl BiDirectionalConnections {
    pub fn new() -> Self {
        Self {
            clients: DashMap::new(),
            servers: DashMap::new(),
        }
    }
}

pub type IDExtractor<C> = Arc<dyn Fn(&C) -> u32 + Send + Sync>;
