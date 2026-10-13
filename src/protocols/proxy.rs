//! Built-in proxy services as mountable protocols.
//!
//! HTTP forward-proxy support lives in the HTTP/1.1 handler (it is decided by
//! the request line), so it is mounted as an HTTP/1.1 protocol with the proxy
//! flags set. SOCKS is its own handler in [`super::socks`]. Both are opt-in and
//! can share a credential gate.

use std::sync::Arc;

use crate::http::router::Router as HttpRouter;
use crate::protocols::http1::{Http1, HttpHandler};
use crate::proxy::ProxyAuthorizer;

/// Build the HTTP/1.1 protocol serving forward-proxy traffic (absolute-form
/// requests / CONNECT) in addition to normal requests.
///
/// The single `"http"` protocol and its one detector serve both modes, so no
/// extra detector registration is needed.
pub fn http_proxy(
    router: Option<Arc<HttpRouter>>,
    handler: Option<HttpHandler>,
    authorizer: Option<ProxyAuthorizer>,
) -> Arc<Http1> {
    Http1::new(router, handler).with_proxy(authorizer).shared()
}