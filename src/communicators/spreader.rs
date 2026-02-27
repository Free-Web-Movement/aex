use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use futures::future::BoxFuture;

pub struct SpreadManager {
    // 存储广播发送端：Map<频道名, Box<broadcast::Sender<T>>>
    hubs: RwLock<HashMap<String, Box<dyn Any + Send + Sync>>>,
}

impl SpreadManager {
    pub fn new() -> Self {
        Self { hubs: RwLock::new(HashMap::new()) }
    }

    /// 【订阅消息】—— 频道的“生命源头”
    /// 只有通过 subscribe，频道才会被物理创建
    pub async fn subscribe<T, F>(&self, name: &str, callback: F) -> Result<(), String>
    where
        T: Clone + Send + Sync + 'static,
        F: Fn(T) -> BoxFuture<'static, ()> + Send + Sync + 'static,
    {
        let mut rx = {
            let mut map = self.hubs.write().await;
            let tx = if let Some(any) = map.get(name) {
                any.downcast_ref::<broadcast::Sender<T>>()
                    .ok_or_else(|| format!("Spread '{}' type mismatch", name))?
                    .clone()
            } else {
                // 只有在这里才初始化频道
                let (tx, _) = broadcast::channel::<T>(1024);
                map.insert(name.to_string(), Box::new(tx.clone()));
                tx
            };
            tx.subscribe()
        };

        let callback = Arc::new(callback);

        // 启动独立监听任务
        tokio::spawn(async move {
            // 注意：broadcast 可能会有 Lagged 错误，这里简单处理
            while let Ok(message) = rx.recv().await {
                let cb = Arc::clone(&callback);
                cb(message).await;
            }
        });

        Ok(())
    }

    /// 【发布消息】—— 纯粹的投递者
    /// 如果频道不存在，意味着没有订阅者，消息直接被“静默丢弃”
    pub async fn publish<T>(&self, name: &str, message: T) -> Result<(), String>
    where
        T: Clone + Send + Sync + 'static,
    {
        let map = self.hubs.read().await;
        
        if let Some(any) = map.get(name) {
            if let Some(tx) = any.downcast_ref::<broadcast::Sender<T>>() {
                // broadcast 的特点：如果没有 Receiver，send 也会成功，但数据会被丢弃
                // 这完美符合广播语义
                let _ = tx.send(message);
                Ok(())
            } else {
                Err(format!("Spread '{}' type mismatch", name))
            }
        } else {
            // 频道不存在，不创建，直接跳过
            // 在广播模型中，这不被视为错误，而是“无人订阅”的状态
            Ok(())
        }
    }
}