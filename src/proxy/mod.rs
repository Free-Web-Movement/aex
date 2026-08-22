//! Built-in proxy services for the unified server.
//!
//! A single listener simultaneously serves a website, an HTTP forward proxy
//! and a SOCKS4/5 proxy. The client's first bytes decide which service line
//! handles the connection — there is no server-side mode switch:
//!
//! ```text
//! client bytes                          service
//! ────────────────────────────────────  ─────────────────────────────
//! "GET / HTTP/1.1"        (origin-form) website router/handlers
//! "GET http://h/ HTTP/1.1"(absolute)    HTTP forward proxy  → upstream
//! "CONNECT host:443"                    HTTP tunnel         → upstream
//! "\x05\x01\x00 ..."                    SOCKS5 CONNECT      → upstream
//! TLS ClientHello                       detector claim → custom handler
//! ```
//!
//! Proxies are opt-in (`UnifiedServer::enable_http_proxy` /
//! `enable_socks_proxy`): an unintentionally open relay is a liability, so
//! nothing is exposed implicitly. An optional authenticator gates both.

pub mod http_proxy;
pub mod socks;

pub use http_proxy::{maybe_handle_http_proxy, ProxyAuthorizer};
pub use socks::{socks_tcp_handler, SocksDetector};
