// --- [GlobalContext] ---

use std::{
    any::{Any, TypeId},
    collections::HashMap,
    net::SocketAddr,
    sync::Arc,
};

use tokio::{
    sync::{Mutex, RwLock},
    task::AbortHandle,
};
use tokio_util::sync::CancellationToken;

use crate::{
    communicators::{
        event::{Event, EventCallback, EventEmitter},
        pipe::{PipeCallback, PipeManager},
        spreader::{SpreadCallback, SpreadManager},
    },
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
    pub exits: Mutex<HashMap<String, (CancellationToken, AbortHandle)>>,
}

impl GlobalContext {
    pub fn new(
        addr: SocketAddr,
        paired_session_keys: Option<Arc<Mutex<PairedSessionKey>>>,
    ) -> Self {
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
            paired_session_keys,
            extensions: Arc::new(RwLock::new(TypeMap::default())),
            routers: TypeMap::default(),
            exits: Mutex::new(HashMap::new()),
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

    pub async fn pipe<T>(&self, name: &str, callback: PipeCallback<T>) -> &Self
    where
        T: Send + 'static,
    {
        self.pipe
            .register(name, callback)
            .await
            .unwrap_or_else(|e| {
                eprintln!("警告: 管道 {} 注册失败: {}", name, e);
            });
        self
    }

    /// 订阅一个全局广播 (1:N)
    pub async fn spread<T>(&self, name: &str, callback: SpreadCallback<T>) -> &Self
    where
        T: Clone + Send + Sync + 'static,
    {
        self.spread
            .subscribe(name, callback)
            .await
            .unwrap_or_else(|e| {
                eprintln!("警告: 广播 {} 订阅失败: {}", name, e);
            });
        self
    }

    /// 监听一个全局事件 (M:N)
    pub async fn event<T>(&self, event_name: &str, callback: EventCallback<T>) -> &Self
    where
        T: Clone + Send + Sync + 'static,
    {
        // 调用我们之前实现的异步版 on
        Event::<T>::_on(&self.event, event_name.to_string(), callback).await;
        self
    }

    /// 🚀 注册服务退出点 (由外部提供 Token 和 Handle)
    pub async fn add_exit(&self, key: &str, token: CancellationToken, handle: AbortHandle) {
        let mut exits = self.exits.lock().await;

        // 健壮性检查：如果 key 已存在，先清理旧的服务避免冲突
        if let Some((old_token, old_handle)) = exits.get(key) {
            old_token.cancel();
            old_handle.abort();
        }

        exits.insert(key.to_string(), (token, handle));
    }

    /// 🔍 获取当前活跃的服务列表
    pub async fn get_exits(&self) -> Vec<String> {
        let exits = self.exits.lock().await;
        exits.keys().cloned().collect()
    }

    /// 🛑 一键全断
    pub async fn shutdown_all(&self) {
        let mut exits = self.exits.lock().await;

        // drain 获取所有权并清空 Map
        for (key, (token, handle)) in exits.drain() {
            // 1. 逻辑退出信号
            token.cancel();
            // 2. 物理强制中止
            handle.abort();

            println!("[AEX] Shutdown component: {}", key);
        }

        // 3. 联动清理 ConnectionManager 里的所有 Peer 连接
        self.manager.shutdown();
    }
}
