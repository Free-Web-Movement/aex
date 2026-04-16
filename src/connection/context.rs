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

use crate::tcp::router::Router as TcpRouter;
use crate::udp::router::Router as UdpRouter;

use crate::connection::global::GlobalContext;
use crate::http::meta::HttpMetadata;
use crate::http::protocol::header::HeaderKey;
use crate::http::protocol::media_type::SubMediaType;
use crate::http::protocol::status::StatusCode;
use crate::http::req::Request;
use crate::http::res::Response;

use crate::constants::server::SERVER_NAME;

/// TypeMap for storing shared data using TypeId as keys. Concurrent version.
pub type ConcurrentTypeMap =
    dashmap::DashMap<std::any::TypeId, Box<dyn std::any::Any + Send + Sync>>;
pub type TypeMap = ConcurrentTypeMap;

/// Non-concurrent TypeMap for per-request storage.
#[derive(Default)]
pub struct LocalTypeMap {
    inner: std::collections::HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl LocalTypeMap {
    pub fn new() -> Self {
        Self {
            inner: std::collections::HashMap::with_capacity(8),
        }
    }

    pub fn get_value<T: Clone + 'static>(&self) -> Option<T> {
        self.inner
            .get(&TypeId::of::<T>())
            .and_then(|boxed_val| boxed_val.downcast_ref::<T>().cloned())
    }

    pub fn get_ref<T: 'static>(&self) -> Option<&T> {
        self.inner
            .get(&TypeId::of::<T>())
            .and_then(|boxed_val| boxed_val.downcast_ref::<T>())
    }

    pub fn get_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.inner
            .get_mut(&TypeId::of::<T>())
            .and_then(|boxed_val| boxed_val.downcast_mut::<T>())
    }

    pub fn set_value<T: Send + Sync + 'static>(&mut self, val: T) {
        self.inner.insert(TypeId::of::<T>(), Box::new(val));
    }
}

/// Router wrappers for type-safe storage
pub struct HttpRouterKey;
pub struct TcpRouterKey;
pub struct UdpRouterKey;

/// Extension trait for ConcurrentTypeMap to get/set values by type.
pub trait TypeMapExt {
    fn get_value<T: Clone + 'static>(&self) -> Option<T>;
    fn set_value<T: Send + Sync + 'static>(&self, val: T);
}

impl TypeMapExt for ConcurrentTypeMap {
    fn get_value<T: Clone + 'static>(&self) -> Option<T> {
        self.get(&TypeId::of::<T>())
            .and_then(|r| r.value().downcast_ref::<T>().cloned())
    }

    fn set_value<T: Send + Sync + 'static>(&self, val: T) {
        self.insert(TypeId::of::<T>(), Box::new(val));
    }
}

/// Get TCP router from global context
pub fn get_tcp_router(global: &ConcurrentTypeMap) -> Option<Arc<crate::tcp::router::Router>> {
    global.get(&TypeId::of::<TcpRouterKey>()).and_then(|r| {
        r.value()
            .downcast_ref::<Arc<crate::tcp::router::Router>>()
            .cloned()
    })
}

/// Get UDP router from global context
pub fn get_udp_router(global: &ConcurrentTypeMap) -> Option<Arc<crate::udp::router::Router>> {
    global.get(&TypeId::of::<UdpRouterKey>()).and_then(|r| {
        r.value()
            .downcast_ref::<Arc<crate::udp::router::Router>>()
            .cloned()
    })
}

pub type AexReader = dyn AsyncBufRead + Send + Sync + Unpin;
pub type AexWriter = dyn AsyncWrite + Send + Sync + Unpin;

pub type BoxReader = Box<AexReader>;
pub type BoxWriter = Box<AexWriter>;

/// Per-request context containing connection info, I/O, and data storage.
pub struct Context {
    pub addr: SocketAddr,
    pub accepted: DateTime<Utc>,
    pub reader: Option<BoxReader>,
    pub writer: Option<BoxWriter>,
    pub global: Arc<GlobalContext>,
    pub local: LocalTypeMap,
}

impl Context {
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
            local: LocalTypeMap::new(),
            addr,
        }
    }

    /// 获取 Request 视图
    pub fn req(&mut self) -> Request<'_> {
        Request {
            reader: &mut self.reader,
            local: &mut self.local,
        }
    }

    /// 获取 Response 视图
    pub fn res(&mut self) -> Response<'_> {
        Response {
            writer: &mut self.writer,
            local: &mut self.local,
        }
    }

    pub fn elapsed(&self) -> u64 {
        Utc::now()
            .signed_duration_since(self.accepted)
            .num_milliseconds()
            .max(0) as u64
    }

    /// 存入扩展实例
    pub fn set<T: Send + Sync + 'static>(&mut self, data: T) {
        self.local.set_value(data);
    }

    /// 获取扩展实例的克隆
    pub fn get<T: Clone + Send + Sync + 'static>(&self) -> Option<T> {
        self.local.get_value::<T>()
    }

    /// Set HTTP status code, returns self for chaining.
    pub fn status(&mut self, code: StatusCode) -> &mut Self {
        if let Some(meta) = self.local.get_mut::<HttpMetadata>() {
            meta.status = code;
        }
        self
    }

    /// Send a response body.
    pub fn send(&mut self, content: impl Into<String>, mime: Option<SubMediaType>) {
        if let Some(meta) = self.local.get_mut::<HttpMetadata>() {
            let bytes: Vec<u8> = content.into().into_bytes();
            let len = bytes.len();

            let mime_str = mime.unwrap_or(SubMediaType::Plain);
            meta.headers
                .insert(HeaderKey::ContentType, mime_str.as_str().to_string());
            meta.headers
                .insert(HeaderKey::ContentLength, len.to_string());
            meta.body = bytes;
        }
    }

    /// Redirect to another URL (302 Found).
    pub fn redirect(&mut self, location: &str) {
        if let Some(meta) = self.local.get_mut::<HttpMetadata>() {
            meta.status = StatusCode::Found;
            meta.headers
                .insert(HeaderKey::Location, location.to_string());
            meta.body = Vec::new();
        }
    }
}
