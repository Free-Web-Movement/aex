use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{ mpsc, RwLock };
use futures::future::BoxFuture;

// 定义回调函数的类型约束
// 它接收一个 T，并返回一个异步的 Unit 结果
type PipeCallback<T> = Box<dyn (Fn(T) -> BoxFuture<'static, ()>) + Send + Sync>;

pub struct PipeManager {
    // 存储发送端，用于 N 端投递
    senders: RwLock<HashMap<String, Box<dyn Any + Send + Sync>>>,
}

impl PipeManager {
    pub fn new() -> Self {
        Self { senders: RwLock::new(HashMap::new()) }
    }

    /// 【接收端注册】
    /// 增加冲突检测：如果 name 已存在，则注册失败并提示错误
    pub async fn register<T, F>(&self, name: &str, callback: F) -> Result<(), String>
        where T: Send + 'static, F: Fn(T) -> BoxFuture<'static, ()> + Send + Sync + 'static
    {
        // 1. 检查名称是否已被占用
        {
            let map = self.senders.read().await;
            if map.contains_key(name) {
                return Err(format!("Pipe registration failed: name '{}' is already in use", name));
            }
        }

        // 2. 获取写锁进行二次检查并插入
        let mut map = self.senders.write().await;
        if map.contains_key(name) {
            return Err(
                format!("Pipe registration failed: name '{}' conflict during race condition", name)
            );
        }

        let (tx, mut rx) = mpsc::unbounded_channel::<T>();
        let callback = Arc::new(callback);

        // 🚀 系统内置机制：启动唯一的消费任务
        tokio::spawn(async move {
            while let Some(message) = rx.recv().await {
                let cb = Arc::clone(&callback);
                cb(message).await;
            }
        });

        map.insert(name.to_string(), Box::new(tx));
        Ok(())
    }

    /// 【发送端投递】
    pub async fn send<T>(&self, name: &str, message: T) -> Result<(), String>
        where T: Send + 'static
    {
        let map = self.senders.read().await;
        if let Some(any_tx) = map.get(name) {
            if let Some(tx) = any_tx.downcast_ref::<mpsc::UnboundedSender<T>>() {
                tx.send(message).map_err(|e| e.to_string())
            } else {
                Err("Type mismatch".into())
            }
        } else {
            Err("Pipe not registered".into())
        }
    }
}
