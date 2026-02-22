use std::collections::HashMap;
use std::sync::Arc;
use std::net::SocketAddr;
use tokio::sync::{Mutex, RwLock};
use tokio::io::{ BufReader, BufWriter };
use tokio::net::tcp::{ OwnedReadHalf, OwnedWriteHalf };

use crate::http::params::Params;
use crate::http::protocol::content_type::ContentType;
use crate::http::protocol::header::HeaderKey;
// 引入你原有的 HTTP 协议相关类型
use crate::http::protocol::method::HttpMethod;

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
                r.value()
                    .downcast_ref::<T>()
                    .map(|v| v.clone())
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
    pub local: TypeMap,
}

// --- HTTP 业务元数据 (存入 local) ---

#[derive(Clone)]
pub struct HttpMetadata {
    pub method: HttpMethod,
    pub path: String,
    pub version: String,
    pub is_chunked: bool,
    pub transfer_encoding: Option<String>,
    pub multipart_boundary: Option<String>,
    pub params: Params,
    pub headers: HashMap<HeaderKey, String>,
    pub content_type: ContentType,
    pub length: usize,
    pub cookies: HashMap<String, String>,
    pub is_websocket: bool,
}

// --- [HTTP 语义化扩展] ---

/// 将 Context 特化为 HTTPContext
/// 这里 R 对应原来的 BufReader<OwnedReadHalf>
/// W 对应原来的 BufWriter<OwnedWriteHalf>
pub type HTTPContext = Context<BufReader<OwnedReadHalf>, BufWriter<OwnedWriteHalf>>;


impl<R, W> Context<R, W> {
    pub fn new(
        reader: R, 
        writer: W, 
        global: Arc<GlobalContext>, 
        addr: SocketAddr
    ) -> Self {
        Self {
            reader,
            writer: Arc::new(Mutex::new(writer)), // 初始化时即包装
            global,
            local: TypeMap::default(),
            addr,
        }
    }
}