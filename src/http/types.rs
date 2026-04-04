use std::sync::Arc;

use futures::future::BoxFuture;

use crate::connection::context::Context;

pub type Executor = dyn for<'a> Fn(&'a mut Context) -> BoxFuture<'a, bool> + Send + Sync;

pub fn to_executor<F>(f: F) -> Arc<Executor>
where
    F: for<'a> Fn(&'a mut Context) -> BoxFuture<'a, bool> + Send + Sync + 'static,
{
    Arc::new(f)
}
