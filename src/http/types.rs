//! # HTTP Types
//!
//! Core types for the HTTP layer including the Executor type.

use std::sync::Arc;

use futures::future::BoxFuture;

use crate::connection::context::Context;

/// Executor is the core type for handling requests and middleware.
///
/// # Signature
///
/// ```ignore
/// Fn(&mut Context) -> BoxFuture<bool>
/// ```
///
/// # Return Value
///
/// - `true`: Continue to the next executor in the chain
/// - `false`: Stop execution, do not call subsequent executors
///
/// # Example
///
/// ```rust,ignore
/// use aex::{body, exe};
///
/// let handler = exe!(|ctx| {
///     // Access request data via ctx.local.get_value::<HttpMetadata>()
///     body!(ctx, "Response body");
///     true  // Continue execution
/// });
/// ```
pub type Executor = dyn for<'a> Fn(&'a mut Context) -> BoxFuture<'a, bool> + Send + Sync;

/// Helper function to convert a closure into an Executor.
pub fn to_executor<F>(f: F) -> Arc<Executor>
where
    F: for<'a> Fn(&'a mut Context) -> BoxFuture<'a, bool> + Send + Sync + 'static,
{
    Arc::new(f)
}
