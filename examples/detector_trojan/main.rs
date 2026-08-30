//! End-to-end trojan-over-TLS demo on a single port.
//!
//! Chain: `TlsDetector` (sniff) → custom_handler("tls") →
//! [`TlsMiddleware::accept`] (terminate TLS with cert/key) →
//! [`TrojanMiddleware::validate`] (parse + strip request header, stash target
//! into ctx.local) → report.
//!
//! Run: `cargo run --example detector_trojan [cert.pem] [key.pem]`
//!
//! A real deployment would check the 56-hex hash against its user database
//! and fall back to a decoy site when validation fails instead of echoing.

#[path = "../detectors_common/mod.rs"]
mod detectors_common;

use std::net::SocketAddr;
use std::sync::Arc;

use detectors_common::{TlsDetector, TlsLoader, TlsMiddleware, TrojanMiddleware, TrojanRequestInfo};
use tokio::io::AsyncWriteExt;

use aex::connection::context::Context;
use aex::connection::global::GlobalContext;
use aex::http::types::Executor;
use aex::unified::UnifiedServer;

async fn run(cert: String, key: String) -> anyhow::Result<()> {
    let loader = TlsLoader::from_paths(&cert, &key)?;

    let addr: SocketAddr = "127.0.0.1:9443".parse()?;
    let globals = Arc::new(GlobalContext::new(addr, None));

    // TLS termination + trojan header parsing as one reusable chain.
    let chain: Arc<Executor> = {
        let loader = loader.clone();
        Arc::new(move |ctx: &mut Context| {
            let tls = TlsMiddleware::accept(loader.clone());
            let trojan = TrojanMiddleware::validate();
            Box::pin(async move {
                if !tls(ctx).await {
                    return false; // TLS handshake failed → drop
                }
                // After decryption the real trojan header is visible.
                trojan(ctx).await
            })
        })
    };

    let server = UnifiedServer::new(addr, globals)
        .detector(Arc::new(TlsDetector))
        .custom_handler(
            "tls",
            Arc::new(move |ctx: Context| {
                let chain = chain.clone();
                tokio::spawn(async move {
                    let mut ctx = ctx;
                    if !chain(&mut ctx).await {
                        return; // invalid TLS or malformed trojan header
                    }
                    let req = ctx.local.get_ref::<TrojanRequestInfo>().cloned();
                    if let Some(w) = ctx.writer.as_mut() {
                        match req {
                            Some(req) => {
                                let _ = w
                                    .write_all(
                                        format!("trojan ok target={}:{}, hash={}\n", req.target, req.port, &req.hash[..12])
                                            .as_bytes(),
                                    )
                                    .await;
                            }
                            None => {
                                let _ = w.write_all(b"trojan ok\n").await;
                            }
                        }
                    }
                })
            }),
        );

    eprintln!("detector_trojan listening on {addr} (cert={cert})");
    server.start().await
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cert = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "cert.pem".to_string());
    let key = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "key.pem".to_string());
    run(cert, key).await
}
