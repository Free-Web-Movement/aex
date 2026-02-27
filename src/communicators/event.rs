use futures::future::{ BoxFuture, FutureExt };
use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

// 1. 依然保留这个方便的别名
pub type EventCallback<D> = Arc<dyn (Fn(D) -> BoxFuture<'static, ()>) + Send + Sync>;

// 2. 改进后的 Event Trait：不再强制要求返回特定类型的 Map
pub trait Event<D> where D: Clone + Send + Sync + 'static {
    /// 这里返回的是擦除类型后的原始 Map
    fn map(&self) -> Arc<RwLock<HashMap<String, Vec<Box<dyn Any + Send + Sync>>>>>;

    /// 注册接收器
    fn on<F>(&self, event_name: String, callback: F) -> impl Future<Output = ()>
        where F: Fn(D) -> BoxFuture<'static, ()> + Send + Sync + 'static
    {
        async {
            let handlers = self.map();
            // 使用异步等待锁，而不是阻塞线程
            let mut map = handlers.write().await;
            let list = map.entry(event_name).or_insert_with(Vec::new);

            let cb: EventCallback<D> = Arc::new(callback);
            list.push(Box::new(cb));
        }
    }

    /// 触发通知
    fn notify(&self, event_name: String, data: D) -> BoxFuture<'static, ()> {
        let handlers_lock = self.map();
        let data = data.clone();

        (
            async move {
                let map = handlers_lock.read().await;
                if let Some(any_callbacks) = map.get(&event_name) {
                    for any_cb in any_callbacks {
                        // 核心逻辑：在这里进行动态类型转换
                        if let Some(cb) = any_cb.downcast_ref::<EventCallback<D>>() {
                            let d = data.clone();
                            let cb_clone = Arc::clone(cb);
                            tokio::spawn(async move {
                                cb_clone(d).await;
                            });
                        }
                        // 如果类型不匹配，这里会静默跳过，这符合多对多的“订阅/过滤”语义
                    }
                }
            }
        ).boxed()
    }
}

// 3. EventEmitter：完全没有泛型，它是系统化的通用容器
pub struct EventEmitter {
    handlers: Arc<RwLock<HashMap<String, Vec<Box<dyn Any + Send + Sync>>>>>,
}

impl EventEmitter {
    pub fn new() -> Self {
        Self {
            handlers: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

// 4. 让 EventEmitter 支持任何符合条件的 D
impl<D> Event<D> for EventEmitter where D: Clone + Send + Sync + 'static {
    fn map(&self) -> Arc<RwLock<HashMap<String, Vec<Box<dyn Any + Send + Sync>>>>> {
        Arc::clone(&self.handlers)
    }
}
