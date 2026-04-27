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

use crate::constants::server::SERVER_NAME;
use crate::{
    communicators::{
        event::{Event, EventCallback, EventEmitter},
        pipe::{PipeCallback, PipeManager},
        spreader::{SpreadCallback, SpreadManager},
    },
    connection::{
        context::TypeMap,
        heartbeat::{HeartbeatConfig, HeartbeatManager},
    },
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
    pub heartbeat_config: HeartbeatConfig,
    pub heartbeat_manager: Option<HeartbeatManager>,
    pub extensions: Arc<RwLock<TypeMap>>,
    pub routers: TypeMap,
    pub h2_codec: std::sync::RwLock<Option<Arc<crate::http2::H2Codec>>>,
    pub exits: Mutex<HashMap<String, (CancellationToken, AbortHandle)>>,
}

impl GlobalContext {
    pub fn new(
        addr: SocketAddr,
        paired_session_keys: Option<Arc<Mutex<PairedSessionKey>>>,
    ) -> Self {
        Self {
            addr,
            local_node: Arc::new(RwLock::new(crate::connection::node::Node::from_addr(
                addr, None, None,
            ))),
            manager: Arc::new(crate::connection::manager::ConnectionManager::new()),
            pipe: PipeManager::default(),
            spread: SpreadManager::default(),
            event: EventEmitter::default(),
            name: SERVER_NAME.to_string(),
            paired_session_keys,
            heartbeat_config: HeartbeatConfig::new(),
            heartbeat_manager: None,
            extensions: Arc::new(RwLock::new(TypeMap::default())),
            routers: TypeMap::default(),
            h2_codec: std::sync::RwLock::new(None),
            exits: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_heartbeat_config(mut self, config: HeartbeatConfig) -> Self {
        self.heartbeat_config = config;
        self
    }

    pub fn init_heartbeat_manager(&mut self) {
        let local_node = futures::executor::block_on(self.local_node.read()).clone();
        self.heartbeat_manager =
            Some(HeartbeatManager::new(local_node).with_config(self.heartbeat_config.clone()));
    }

    pub fn start_heartbeat(
        &self,
        ctx: Arc<tokio::sync::Mutex<crate::connection::context::Context>>,
        peer_addr: SocketAddr,
        cancel_token: CancellationToken,
    ) {
        if let Some(ref manager) = self.heartbeat_manager {
            let manager = manager.clone();
            let _ = manager.start_server_heartbeat(ctx, peer_addr, cancel_token);
        }
    }

    pub fn set_server_name(&mut self, name: String) {
        self.name = name;
    }

    /// 存入扩展实例 (Async)
    pub async fn set<T: Clone + Send + Sync + 'static>(&self, data: T) {
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
            .unwrap_or_else(|_e| {
                tracing::warn!("Pipe registration failed: {}", name);
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
            .unwrap_or_else(|_e| {
                tracing::warn!("Broadcast subscription failed: {}", name);
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

            tracing::info!("Shutdown component: {}", key);
        }

        // 3. 联动清理 ConnectionManager 里的所有 Peer 连接
        self.manager.shutdown();
    }

    pub fn get_connection_info(&self) -> ConnectionInfo {
        let mut inbound = Vec::new();
        let mut outbound = Vec::new();

        for bucket_ref in self.manager.connections.iter() {
            let scope = bucket_ref.key().1;

            for entry_ref in bucket_ref.servers.iter() {
                let addr = *entry_ref.key();
                let entry = entry_ref.value();
                inbound.push(PeerInfo {
                    addr: addr.to_string(),
                    direction: "inbound".to_string(),
                    scope: format!("{:?}", scope),
                    uptime_secs: entry.uptime_secs(),
                });
            }

            for entry_ref in bucket_ref.clients.iter() {
                let addr = *entry_ref.key();
                let entry = entry_ref.value();
                outbound.push(PeerInfo {
                    addr: addr.to_string(),
                    direction: "outbound".to_string(),
                    scope: format!("{:?}", scope),
                    uptime_secs: entry.uptime_secs(),
                });
            }
        }

        ConnectionInfo { inbound, outbound }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ConnectionInfo {
    pub inbound: Vec<PeerInfo>,
    pub outbound: Vec<PeerInfo>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PeerInfo {
    pub addr: String,
    pub direction: String,
    pub scope: String,
    pub uptime_secs: u64,
}
