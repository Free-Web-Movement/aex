// --- [GlobalContext] ---

use std::{
    net::SocketAddr,
    sync::{Arc, RwLock},
};

use crate::{
    communicators::{event::EventEmitter, pipe::PipeManager, spreader::SpreadManager},
    connection::context::{SERVER_NAME, TypeMap},
    crypto::session_key_manager::PairedSessionKey,
};

pub struct GlobalContext {
    // local socket listening address
    pub addr: SocketAddr,
    pub local_node: Arc<RwLock<crate::connection::node::Node>>,
    pub manager: Arc<crate::connection::manager::ConnectionManager>,
    pub pipe: PipeManager,
    pub spread: SpreadManager,
    pub event: EventEmitter,
    pub name: String,
    pub paired_session_keys: Option<PairedSessionKey>,
    /// 全局 TypeMap：允许灵活添加数据库连接池、全局配置等
    pub extensions: Arc<RwLock<TypeMap>>,
}

impl GlobalContext {
    pub fn new(addr: SocketAddr) -> Self {
        Self {
            addr,
            // 假设 Node 和 ConnectionManager 都有默认初始化方法
            local_node: Arc::new(RwLock::new(crate::connection::node::Node::from_addr(
                addr, None, None,
            ))),
            manager: Arc::new(crate::connection::manager::ConnectionManager::new()),
            pipe: PipeManager::default(),
            spread: SpreadManager::default(),
            event: EventEmitter::default(),
            name: SERVER_NAME.to_string(),
            paired_session_keys: None,
            extensions: Arc::new(RwLock::new(TypeMap::default())),
        }
    }

    pub fn set_server_name(&mut self, name: String) {
        self.name = name;
    }
}
