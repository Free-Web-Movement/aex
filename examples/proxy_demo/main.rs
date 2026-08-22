//! One port, three services demo.
//!
//! ```text
//! cargo run --features proxy --example proxy_demo
//!
//! # website
//! curl http://127.0.0.1:8080/
//!
//! # HTTP forward proxy
//! curl -x http://127.0.0.1:8080 http://<any-origin>/path
//!
//! # SOCKS5 proxy (add --socks5-user alice --socks5-pass secret for auth)
//! curl --socks5-hostname 127.0.0.1:8080 http://<any-origin>/path
//! ```

use std::net::SocketAddr;
use std::sync::Arc;

use aex::connection::context::Context;
use aex::connection::global::GlobalContext;
use aex::unified::UnifiedServer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let addr: SocketAddr = "127.0.0.1:8080".parse()?;
    let server = UnifiedServer::new(addr, Arc::new(GlobalContext::new(addr, None)))
        .enable_proxies()
        .proxy_authenticator(Arc::new(|user, pass| {
            // Demo credentials; replace with your user database lookup.
            user == "alice" && pass == "secret"
        }))
        .http_handler(Arc::new(|ctx: &mut Context| {
            Box::pin(async move {
                if let Some(meta) = ctx.local.get_mut::<aex::http::meta::HttpMetadata>() {
                    meta.body = b"hello from the website side\n".to_vec();
                }
                true
            })
        }));

    eprintln!("proxy_demo listening on {addr} (website + http proxy + socks5)");
    server.start().await
}
