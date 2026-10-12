//! Built-in proxy services as mountable protocols.
//!
//! HTTP forward-proxy support lives in the HTTP/1.1 handler (it is decided by
//! the request line), so it is mounted as an HTTP/1.1 protocol with the proxy
//! flags set. SOCKS is its own handler in [`super::socks`]. Both are opt-in and
//! can share a credential gate.

use std::sync::Arc;

use bytes::Bytes;

use crate::protocols::http1::Http1Config;
use crate::proxy::ProxyAuthorizer;
use crate::unified::handler::ProtocolHandler;

/// Build the HTTP/1.1 protocol serving forward-proxy traffic (absolute-form
/// requests / CONNECT) in addition to normal requests.
///
/// The single `"http"` protocol and its one detector serve both modes, so no
/// extra detector registration is needed.
pub fn http_proxy(
    cfg: Http1Config,
    authorizer: Option<ProxyAuthorizer>,
) -> Arc<ProtocolHandler<Bytes>> {
    let mut cfg = cfg;
    cfg.http_proxy_enabled = true;
    cfg.proxy_authorizer = authorizer;
    crate::protocols::http1::protocol(cfg)
}
