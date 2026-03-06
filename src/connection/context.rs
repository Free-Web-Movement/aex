use chrono::DateTime;
use chrono::Utc;
use std::any::Any;
use std::any::TypeId;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::AsyncBufRead;
use tokio::io::AsyncWrite;

use crate::connection::global::GlobalContext;
use crate::http::req::Request;
use crate::http::res::Response;

pub const SERVER_NAME: &str = "Aex/1.0";

/// 全局扩展存储（TypeMap 抽象）
pub type TypeMap = dashmap::DashMap<std::any::TypeId, Box<dyn std::any::Any + Send + Sync>>;

pub trait TypeMapExt {
    fn get_value<T: Clone + 'static>(&self) -> Option<T>;
    fn set_value<T: Send + Sync + 'static>(&self, val: T);
}

impl TypeMapExt for TypeMap {
    /// 这里的 get 是基于 TypeId 的查找，而不是基于 Key 值的查找
    fn get_value<T: Clone + 'static>(&self) -> Option<T> {
        self.get(&TypeId::of::<T>()) // 传入 TypeId 作为 Key
            .and_then(|r| {
                // r.value() 拿到的是 Box<dyn Any>
                r.value().downcast_ref::<T>().cloned()
            })
    }

    fn set_value<T: Send + Sync + 'static>(&self, val: T) {
        self.insert(TypeId::of::<T>(), Box::new(val));
    }
}

// --- 基础 Trait 组合 (不带 Box) ---
// 虽然 Rust 稳定版不能直接定义 trait Alias，但我们可以定义这些别名用于 dyn
pub type AexReader = dyn AsyncBufRead + Send + Sync + Unpin;
pub type AexWriter = dyn AsyncWrite + Send + Sync + Unpin;

// --- 包装后的类型 (带 Box) ---
pub type BoxReader = Box<AexReader>;
pub type BoxWriter = Box<AexWriter>;
// --- [Context] ---
pub struct Context<'a> {
    pub addr: SocketAddr,
    pub accepted: DateTime<Utc>,
    // ⚡ 统一使用 dyn 包装，不再需要 R 和 W 泛型位
    pub reader: &'a mut Option<BoxReader>,
    pub writer: &'a mut Option<BoxWriter>,
    pub global: Arc<GlobalContext>,
    pub local: Arc<TypeMap>,
}

impl<'a> Context<'a> {
    // ⚡ 构造函数：接受外部已经包装好的 Option 引用
    pub fn new(
        reader: &'a mut Option<BoxReader>,
        writer: &'a mut Option<BoxWriter>,
        global: Arc<GlobalContext>,
        addr: SocketAddr,
    ) -> Self {
        Self {
            accepted: Utc::now(),
            reader,
            writer,
            global,
            local: Arc::new(TypeMap::default()),
            addr,
        }
    }

    /// 获取 Request 视图
    pub fn req(&mut self) -> Request<'_> {
        Request {
            // ⚡ 这里透传 &mut Option，Request 内部决定是 read 还是 take()
            reader: self.reader,
            local: self.local.clone(),
        }
    }

    /// 获取 Response 视图
    pub fn res(&mut self) -> Response<'_> {
        Response {
            writer: self.writer,
            local: self.local.clone(),
        }
    }

    pub fn elapsed(&self) -> u64 {
        Utc::now()
            .signed_duration_since(self.accepted)
            .num_milliseconds()
            .max(0) as u64
    }

    /// 存入扩展实例 (Async)
    pub async fn set<T: Send + Sync + 'static>(&self, data: T) {
        let key = TypeId::of::<T>();
        let value: Box<dyn Any + Send + Sync> = Box::new(data);
        self.local.insert(key, value);
    }

    /// 获取扩展实例的克隆 (Async)
    pub async fn get<T: Clone + Send + Sync + 'static>(&self) -> Option<T> {
        let key = TypeId::of::<T>();
        self.local
            .get(&key)
            .and_then(|boxed_val| boxed_val.downcast_ref::<T>().cloned())
    }
}
