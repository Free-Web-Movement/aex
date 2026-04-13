//! # HTTP Types
//!
//! Core types for the HTTP layer including the Executor type.

use std::sync::Arc;

use futures::future::BoxFuture;

use crate::connection::context::Context;

/// Executor is the core type for handling requests and middleware.
pub type Executor = dyn for<'a> Fn(&'a mut Context) -> BoxFuture<'a, bool> + Send + Sync;

/// Route handler type alias
pub type RouteHandler = Arc<Executor>;

/// Middleware chain type alias
pub type MiddlewareChain = Vec<Arc<Executor>>;

/// Helper function to convert a closure into an Executor.
pub fn to_executor<F>(f: F) -> Arc<Executor>
where
    F: for<'a> Fn(&'a mut Context) -> BoxFuture<'a, bool> + Send + Sync + 'static,
{
    Arc::new(f)
}
