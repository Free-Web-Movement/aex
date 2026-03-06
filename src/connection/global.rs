// --- [GlobalContext] ---

use std::{
    any::{Any, TypeId},
    net::SocketAddr,
    sync::Arc,
};

use tokio::sync::{Mutex, RwLock};

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
    pub paired_session_keys: Option<Arc<Mutex<PairedSessionKey>>>,
    /// 全局 TypeMap：允许灵活添加数据库连接池、全局配置等
    pub extensions: Arc<RwLock<TypeMap>>,
    pub routers: TypeMap,
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
            routers: TypeMap::default(),
        }
    }

    pub fn set_server_name(&mut self, name: String) {
        self.name = name;
    }

    /// 存入扩展实例 (Async)
    pub async fn set<T: Send + Sync + 'static>(&self, data: T) {
        // 1. 获取异步写锁（注意：tokio 的 write() 不需要 .expect()）
        let ext = self.extensions.write().await;

        // 2. 计算 TypeId 并包装数据
        let key = TypeId::of::<T>();
        let value: Box<dyn Any + Send + Sync> = Box::new(data);

        // 3. 插入 DashMap
        ext.insert(key, value);
    }

    /// 获取扩展实例的克隆 (Async)
    pub async fn get<T: Clone + Send + Sync + 'static>(&self) -> Option<T> {
        // 1. 获取异步读锁
        let ext = self.extensions.read().await;

        // 2. 查找并尝试向下转型
        let key = TypeId::of::<T>();
        ext.get(&key).and_then(|boxed_val| {
            // boxed_val 是 DashMap 的引用，指向 Box<dyn Any...>
            boxed_val.downcast_ref::<T>().cloned()
        })
    }
}
