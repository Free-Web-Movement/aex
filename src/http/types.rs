use std::sync::Arc;

use futures::future::BoxFuture;

use crate::{connection::context::HTTPContext, http::middlewares::websocket::WebSocket};

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
