use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{BufReader, BufWriter};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::{Mutex, RwLock};

use crate::connection::req::Request;
use crate::connection::res::Response;
use crate::http::params::Params;
use crate::http::protocol::content_type::ContentType;
use crate::http::protocol::header::HeaderKey;
// 引入你原有的 HTTP 协议相关类型
use crate::http::protocol::method::HttpMethod;
use crate::http::protocol::status::StatusCode;
use crate::http::protocol::version::HttpVersion;
use crate::server::SERVER_NAME;

/// 全局扩展存储（TypeMap 抽象）
pub type TypeMap = dashmap::DashMap<std::any::TypeId, Box<dyn std::any::Any + Send + Sync>>;

use std::any::TypeId;

// 假设你的 TypeMap 定义如下：
// pub type TypeMap = dashmap::DashMap<TypeId, Box<dyn std::any::Any + Send + Sync>>;

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
                r.value().downcast_ref::<T>().map(|v| v.clone())
            })
    }

    fn set_value<T: Send + Sync + 'static>(&self, val: T) {
        self.insert(TypeId::of::<T>(), Box::new(val));
    }
}

// --- [GlobalContext] ---

pub struct GlobalContext {
    // local socket listening address
    pub addr: SocketAddr,
    pub local_node: Arc<RwLock<crate::connection::node::Node>>,
    pub manager: Arc<crate::connection::manager::ConnectionManager>,
    /// 全局 TypeMap：允许灵活添加数据库连接池、全局配置等
    pub extensions: Arc<RwLock<TypeMap>>,
}

// --- [Context] ---
pub type SharedWriter<W> = Arc<Mutex<W>>;
/// 泛型 Context：AEX 的核心，R 和 W 代表读写流
pub struct Context<R, W> {
    // remote socket address
    pub addr: SocketAddr,
    pub reader: R,
    pub writer: SharedWriter<W>,
    pub global: Arc<GlobalContext>,
    /// 本地 TypeMap：用于存储请求级别的临时变量
    // pub local: TypeMap,
    pub meta_in: HttpMetadata, // 供中间件和处理器使用的输入元数据
    pub meta_out: HttpMetadata, // 供中间件和处理器使用
}

// --- HTTP 业务元数据 (存入 local) ---

// 常规的HTTP请求元数据，供中间件和处理器使用
#[derive(Debug, Clone)]
pub struct HttpMetadata {
    pub method: HttpMethod,
    pub path: String,
    pub version: HttpVersion,
    pub is_chunked: bool,
    pub transfer_encoding: Option<String>,
    pub multipart_boundary: Option<String>,
    pub params: Option<Params>, // 放在Trie路由里解析
    pub headers: HashMap<HeaderKey, String>,
    pub content_type: ContentType,
    pub length: usize,
    pub cookies: HashMap<String, String>,
    pub is_websocket: bool,
    pub server: String,
    //
    pub status: StatusCode, // 处理结果状态码，默认200

    // 如果是form-url-encoded的请求，form会被保存在Params里面
    // body的具体实现不同，请求需要不同的body处理方式（如chunked、websocket等），
    // 所以不直接放在HttpMetadata里，而是根据需要在中间件里动态解析和存储
    pub body: Vec<u8>, // 处理结果消息体（如验证错误信息等），默认空
}

// --- [HTTP 语义化扩展] ---

/// 将 Context 特化为 HTTPContext
/// 这里 R 对应原来的 BufReader<OwnedReadHalf>
/// W 对应原来的 BufWriter<OwnedWriteHalf>
pub type HTTPContext = Context<BufReader<OwnedReadHalf>, BufWriter<OwnedWriteHalf>>;

impl<R, W> Context<R, W> {
    pub fn new(reader: R, writer: W, global: Arc<GlobalContext>, addr: SocketAddr) -> Self {
        Self {
            reader,
            writer: Arc::new(Mutex::new(writer)), // 初始化时即包装
            global,
            meta_in: HttpMetadata::default(),
            meta_out: HttpMetadata::default(),
            // local: TypeMap::default(),
            addr,
        }
    }

    /// 构造并返回 Request 视图
    /// 注意：由于 R 通常在 Mutex 中，这里需要处理锁的生命周期或传入 Guard
    pub async fn req<'a>(&'a mut self) -> Request<'a, R> {
        Request {
            reader: &mut self.reader,
            meta: &mut self.meta_in, // local: &self.local,
        }
    }

    /// 构造并返回 Response 视图
    pub fn res<'a>(&'a mut self) -> Response<'a, W> {
        Response {
            writer: &mut self.writer,
            meta: &mut self.meta_out,
        }
    }
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
            extensions: Arc::new(RwLock::new(TypeMap::default())),
        }
    }
}

impl Default for HttpMetadata {
    fn default() -> Self {
        Self {
            method: HttpMethod::GET, // 默认 GET
            path: "/".to_string(),
            version: HttpVersion::Http11,
            is_chunked: false,
            transfer_encoding: None,
            multipart_boundary: None,
            params: None,
            headers: HashMap::new(),
            // 假设 ContentType 有默认值（通常是 text/plain 或 application/octet-stream）
            content_type: ContentType::default(),
            length: 0,
            cookies: HashMap::new(),
            is_websocket: false,
            server: SERVER_NAME.to_string(),
            status: StatusCode::Ok, // 默认 200 OK
            body: Vec::new(),
        }
    }
}

impl HttpMetadata {
    /// 创建一个基础的元数据对象
    pub fn new() -> Self {
        Self::default()
    }
}
