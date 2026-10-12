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
use aex::protocols::http1::{Http1Config, HttpHandler};
use aex::proxy::ProxyAuthorizer;
use aex::unified::UnifiedServer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let addr: SocketAddr = "127.0.0.1:8080".parse()?;

    // Demo credentials; replace with your user database lookup.
    let authorizer: ProxyAuthorizer = Arc::new(|user, pass| user == "alice" && pass == "secret");

    let website: HttpHandler = Arc::new(|ctx: &mut Context| {
        Box::pin(async move {
            if let Some(meta) = ctx.local.get_mut::<aex::http::meta::HttpMetadata>() {
                meta.body = b"hello from the website side\n".to_vec();
            }
            true
        })
    });

    let server = UnifiedServer::at(addr, Arc::new(GlobalContext::new(addr, None)))
        // One HTTP/1.1 protocol serves both the website (origin-form requests)
        // and the HTTP forward proxy (absolute-form / CONNECT), gated by the
        // same credentials.
        .push(aex::protocols::proxy::http_proxy(
            Http1Config {
                handler: Some(website),
                ..Default::default()
            },
            Some(authorizer.clone()),
        ))
        .push(aex::protocols::socks::protocol(Some(authorizer)));

    eprintln!("proxy_demo listening on {addr} (website + http proxy + socks5)");
    server.start().await
}
