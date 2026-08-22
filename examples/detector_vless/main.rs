//! VLESS-over-TLS structural validation demo.
//!
//! Same layering as the trojan example but with [`VlessMiddleware`]: after
//! TLS termination the plaintext VLESS header is parsed, stripped and its
//! fields (UUID, command, target) are reported.
//!
//! NOTE: structural parsing does NOT authenticate — a real server must check
//! `VlessRequestInfo::user` against an access list and reject unknown UUIDs.
//!
//! Run: `cargo run --example detector_vless [cert.pem] [key.pem]`

#[path = "../detectors_common/mod.rs"]
mod detectors_common;

use std::net::SocketAddr;
use std::sync::Arc;

use detectors_common::{TlsDetector, TlsLoader, TlsMiddleware, VlessMiddleware, VlessRequestInfo};
use tokio::io::AsyncWriteExt;

use aex::connection::context::Context;
use aex::connection::global::GlobalContext;
use aex::http::types::Executor;
use aex::unified::UnifiedServer;

fn uuid_str(b: &[u8; 16]) -> String {
    let h = |r: std::ops::Range<usize>| {
        b[r].iter()
            .map(|x| format!("{x:02x}"))
            .collect::<String>()
    };
    format!(
        "{}-{}-{}-{}-{}",
        h(0..4),
        h(4..6),
        h(6..8),
        h(8..10),
        h(10..16)
    )
}

async fn run(cert: String, key: String) -> anyhow::Result<()> {
    let loader = TlsLoader::from_paths(&cert, &key)?;

    let addr: SocketAddr = "127.0.0.1:9444".parse()?;
    let globals = Arc::new(GlobalContext::new(addr, None));

    let chain: Arc<Executor> = {
        let loader = loader.clone();
        Arc::new(move |ctx: &mut Context| {
            let tls = TlsMiddleware::accept(loader.clone());
            let vless = VlessMiddleware::validate();
            Box::pin(async move {
                if !tls(ctx).await {
                    return false;
                }
                if !vless(ctx).await {
                    return false;
                }
                // Production: verify UUID here before serving.
                true
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
                        return;
                    }
                    if let (Some(w), Some(req)) =
                        (ctx.writer.as_mut(), ctx.local.get_ref::<VlessRequestInfo>().cloned())
                    {
                        let _ = w
                            .write_all(
                                format!(
                                    "vless ok user={} cmd={} target={}\n",
                                    uuid_str(&req.user),
                                    req.command,
                                    req.target
                                )
                                .as_bytes(),
                            )
                            .await;
                    }
                })
            }),
        );

    eprintln!("detector_vless listening on {addr} (cert={cert})");
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
