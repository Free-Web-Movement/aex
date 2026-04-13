use std::{net::SocketAddr, sync::Arc};

use dashmap::DashMap;

use crate::connection::entry::ConnectionEntry;

#[derive(Debug, Clone)]
pub struct BiDirectionalConnections {
    pub clients: DashMap<SocketAddr, Arc<ConnectionEntry>>,
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

pub type ConnectionMap = DashMap<SocketAddr, Arc<ConnectionEntry>>;
