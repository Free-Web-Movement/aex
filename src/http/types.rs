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

/// The value a route handler produces.
///
/// Implemented for:
/// - `bool`: the handler controls the result directly
/// - `()`: success with no body — the plain `|ctx| { ... }` form
/// - `String` / `&str`: the returned string is sent as the response body
///
/// so handlers can be written as `router.get("/", |_ctx| "Hello")` or
/// `router.get("/", |ctx| { ctx.text("Hello"); })`.
pub trait HandlerOutput {
    fn into_boxed(self, ctx: &mut Context) -> BoxFuture<'static, bool>;
}

impl HandlerOutput for bool {
    fn into_boxed(self, _ctx: &mut Context) -> BoxFuture<'static, bool> {
        Box::pin(futures::future::ready(self))
    }
}

impl HandlerOutput for () {
    fn into_boxed(self, _ctx: &mut Context) -> BoxFuture<'static, bool> {
        Box::pin(futures::future::ready(true))
    }
}

impl HandlerOutput for String {
    fn into_boxed(self, ctx: &mut Context) -> BoxFuture<'static, bool> {
        ctx.send(self, None);
        Box::pin(futures::future::ready(true))
    }
}

impl HandlerOutput for &'static str {
    fn into_boxed(self, ctx: &mut Context) -> BoxFuture<'static, bool> {
        ctx.send(self, None);
        Box::pin(futures::future::ready(true))
    }
}

/// Converts a middleware into an `Arc<Executor>`.
///
/// Implemented for pre-built `Arc<Executor>` values (e.g. validators) and for
/// plain closures, so `.middleware(mw)` accepts both.
pub trait IntoExecutor {
    fn into_executor(self) -> Arc<Executor>;
}

impl<F, R> IntoExecutor for F
where
    F: for<'a> Fn(&'a mut Context) -> R + Send + Sync + 'static,
    R: HandlerOutput + Send + 'static,
{
    fn into_executor(self) -> Arc<Executor> {
        Arc::new(move |ctx: &mut Context| Box::pin(self(ctx).into_boxed(ctx)))
    }
}

impl<F, R> IntoExecutor for Arc<F>
where
    F: for<'a> Fn(&'a mut Context) -> R + Send + Sync + 'static,
    R: HandlerOutput + Send + 'static,
{
    fn into_executor(self) -> Arc<Executor> {
        Arc::new(move |ctx: &mut Context| Box::pin(self(ctx).into_boxed(ctx)))
    }
}

impl IntoExecutor for Arc<Executor> {
    fn into_executor(self) -> Arc<Executor> {
        self
    }
}
