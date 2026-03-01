use chrono::DateTime;
use chrono::Utc;
use tokio::sync::RwLock;
use std::any::TypeId;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{BufReader, BufWriter};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::{Mutex};

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

// --- [Context] ---
pub type SharedWriter<W> = Arc<Mutex<W>>;
/// 泛型 Context：AEX 的核心，R 和 W 代表读写流
pub struct Context<R, W> {
    // remote socket address
    pub addr: SocketAddr,
    // 连接被进入时间
    pub accepted: DateTime<Utc>,
    pub reader: R,
    pub writer: SharedWriter<W>,
    pub global: Arc<RwLock<GlobalContext>>,
    /// 本地 TypeMap：用于存储请求级别的临时变量
    pub local: TypeMap,
}

// --- HTTP 业务元数据 (存入 local) ---

// --- [HTTP 语义化扩展] ---

/// 将 Context 特化为 HTTPContext
/// 这里 R 对应原来的 BufReader<OwnedReadHalf>
/// W 对应原来的 BufWriter<OwnedWriteHalf>
pub type HTTPContext = Context<BufReader<OwnedReadHalf>, BufWriter<OwnedWriteHalf>>;

impl<R, W> Context<R, W> {
    pub fn new(reader: R, writer: W, global: Arc<RwLock<GlobalContext>>, addr: SocketAddr) -> Self {
        Self {
            accepted: Utc::now(),
            reader,
            writer: Arc::new(Mutex::new(writer)), // 初始化时即包装
            global,
            local: TypeMap::default(),
            addr,
        }
    }

    /// 构造并返回 Request 视图
    /// 注意：由于 R 通常在 Mutex 中，这里需要处理锁的生命周期或传入 Guard
    pub async fn req<'a>(&'a mut self) -> Request<'a, R> {
        Request {
            reader: &mut self.reader,
            local: &mut self.local,
        }
    }

    /// 构造并返回 Response 视图
    pub fn res<'a>(&'a mut self) -> Response<'a, W> {
        Response {
            writer: &mut self.writer,
            local: &mut self.local,
        }
    }

    /// 毫秒表示的已经经历时间
    pub fn elapsed(&self) -> u64 {
        Utc::now()
            .signed_duration_since(self.accepted)
            .num_milliseconds()
            .max(0) as u64
    }
}
