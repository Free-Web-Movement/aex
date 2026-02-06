use std::{ any::{ Any, TypeId }, collections::HashMap, sync::Arc };

use futures::future::BoxFuture;

use crate::{ req::Request, res::Response, websocket::WebSocket };

// HTTP 上下文
pub struct HTTPContext {
    pub req: Request,
    pub res: Response,
    pub global: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
    pub local: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

// pub type Executor = fn(&mut HTTPContext) -> bool;
pub type Executor =
    dyn for<'a> Fn(&'a mut HTTPContext) -> BoxFuture<'a, bool> + Send + Sync;

pub type TextHandler = Arc<
    dyn for<'a> Fn(&WebSocket, &'a mut HTTPContext, String) -> BoxFuture<'a, bool> + Send + Sync
>;
pub type BinaryHandler = Arc<
   dyn for<'a> Fn(&WebSocket, &'a mut HTTPContext, Vec<u8>) -> BoxFuture<'a, bool> + Send + Sync
>;