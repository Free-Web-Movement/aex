use std::{
    any::{Any, TypeId}, collections::HashMap, net::SocketAddr, pin::Pin, sync::Arc
};

use futures::future::BoxFuture;
use tokio::{net::{UdpSocket, tcp::{OwnedReadHalf, OwnedWriteHalf}}, sync::Mutex};

use crate::{req::Request, res::Response, middlewares::websocket::WebSocket};

pub trait ContextKey: 'static {
    type Value: 'static;
}
pub struct TypeMap {
    map: HashMap<TypeId, Box<dyn Any + Send>>,
}

impl TypeMap {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub fn set<K: ContextKey>(&mut self, value: K::Value) where <K as ContextKey>::Value: std::marker::Send {
        self.map.insert(TypeId::of::<K>(), Box::new(value));
    }

    pub fn get<K: ContextKey>(&self) -> Option<&K::Value> {
        self.map
            .get(&TypeId::of::<K>())
            .and_then(|v| v.downcast_ref())
    }

    pub fn get_mut<K: ContextKey>(&mut self) -> Option<&mut K::Value> {
        self.map
            .get_mut(&TypeId::of::<K>())
            .and_then(|v| v.downcast_mut())
    }
}

// HTTP 上下文
pub struct HTTPContext {
    pub req: Request,
    pub res: Response,
    pub global: Arc<Mutex<TypeMap>>,
    pub local: TypeMap,
}

// pub type Executor = fn(&mut HTTPContext) -> bool;
pub type Executor = dyn for<'a> Fn(&'a mut HTTPContext) -> BoxFuture<'a, bool> + Send + Sync;

pub type TextHandler = Arc<
    dyn for<'a> Fn(&WebSocket, &'a mut HTTPContext, String) -> BoxFuture<'a, bool> + Send + Sync,
>;
pub type BinaryHandler = Arc<
    dyn for<'a> Fn(&WebSocket, &'a mut HTTPContext, Vec<u8>) -> BoxFuture<'a, bool> + Send + Sync,
>;

pub fn to_executor<F>(f: F) -> Arc<Executor>
where
    F: for<'a> Fn(&'a mut HTTPContext) -> BoxFuture<'a, bool>
        + Send
        + Sync
        + 'static,
{
    Arc::new(f)
}


use bytes::BytesMut;

/// C: 业务指令/数据对象 (Command/Message)
pub trait Codec: Sized {
    fn decode(src: &mut BytesMut) -> Option<Self>;
    fn encode(self, dst: &mut BytesMut);
}

pub type StreamExecutor = Box<dyn Fn(OwnedReadHalf, OwnedWriteHalf) -> Pin<Box<dyn Future<Output = anyhow::Result<bool>> + Send>> + Send + Sync>;

pub type PacketExecutor = Box<dyn Fn(Vec<u8>, SocketAddr, Arc<UdpSocket>) -> Pin<Box<dyn Future<Output = anyhow::Result<bool>> + Send>> 
    + Send 
    + Sync
>;