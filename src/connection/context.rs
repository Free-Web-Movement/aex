//! # Connection Context
//!
//! Per-request context for handling HTTP/TCP/UDP connections.
//!
//! ## Context Structure
//!
//! - `local`: Per-request TypeMap storage for request/response data
//! - `global`: Shared state across all connections
//! - `reader`/`writer`: I/O streams for the connection

use chrono::DateTime;
use chrono::Utc;
use std::any::Any;
use std::any::TypeId;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::AsyncBufRead;
use tokio::io::AsyncWrite;

use crate::connection::global::GlobalContext;
use crate::http::meta::HttpMetadata;
use crate::http::protocol::header::HeaderKey;
use crate::http::req::Request;
use crate::http::res::Response;

pub const SERVER_NAME: &str = "Aex/1.0";

/// TypeMap for storing per-request or shared data using TypeId as keys.
///
/// # Example
///
/// ```rust,ignore
/// ctx.local.set_value::<MyData>(my_data);
/// let data = ctx.local.get_value::<MyData>();
/// ```
pub type TypeMap = dashmap::DashMap<std::any::TypeId, Box<dyn std::any::Any + Send + Sync>>;

/// Extension trait for TypeMap to get/set values by type.
pub trait TypeMapExt {
    fn get_value<T: Clone + 'static>(&self) -> Option<T>;
    fn set_value<T: Send + Sync + 'static>(&self, val: T);
}

impl TypeMapExt for TypeMap {
    fn get_value<T: Clone + 'static>(&self) -> Option<T> {
        self.get(&TypeId::of::<T>())
            .and_then(|r| r.value().downcast_ref::<T>().cloned())
    }

    fn set_value<T: Send + Sync + 'static>(&self, val: T) {
        self.insert(TypeId::of::<T>(), Box::new(val));
    }
}

pub type AexReader = dyn AsyncBufRead + Send + Sync + Unpin;
pub type AexWriter = dyn AsyncWrite + Send + Sync + Unpin;

pub type BoxReader = Box<AexReader>;
pub type BoxWriter = Box<AexWriter>;

/// Per-request context containing connection info, I/O, and data storage.
///
/// # Fields
///
/// - `addr`: Remote socket address
/// - `accepted`: Connection acceptance timestamp
/// - `reader`: Input stream (for reading request data)
/// - `writer`: Output stream (for writing response data)
/// - `global`: Shared global context
/// - `local`: Per-request TypeMap storage
pub struct Context {
    pub addr: SocketAddr,
    pub accepted: DateTime<Utc>,
    pub reader: Option<BoxReader>,
    pub writer: Option<BoxWriter>,
    pub global: Arc<GlobalContext>,
    pub local: Arc<TypeMap>,
}

impl Context {
    // ⚡ 构造函数：接受外部已经包装好的 Option 引用
    pub fn new(
        reader: Option<BoxReader>,
        writer: Option<BoxWriter>,
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
            reader: &mut self.reader,
            local: self.local.clone(),
        }
    }

    /// 获取 Response 视图
    pub fn res(&mut self) -> Response<'_> {
        Response {
            writer: &mut self.writer,
            local: self.local.clone(),
        }
    }

    pub fn elapsed(&self) -> u64 {
        Utc::now()
            .signed_duration_since(self.accepted)
            .num_milliseconds()
            .max(0) as u64
    }

    /// 存入扩展实例
    pub fn set<T: Send + Sync + 'static>(&self, data: T) {
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

    /// Send a response body, replacing body! macro.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// ctx.send("Hello, World!");
    /// ctx.send(format!("User: {}", name));
    /// ctx.send(r#"{"status":"ok"}"#);
    /// ```
    pub fn send(&self, content: impl Into<String>) {
        let mut meta = self.local.get_value::<HttpMetadata>().unwrap();
        let bytes: Vec<u8> = content.into().into_bytes();
        let len = bytes.len();

        meta.headers.insert(HeaderKey::ContentLength, len.to_string());
        meta.body = bytes;
        self.local.set_value::<HttpMetadata>(meta);
    }
}
