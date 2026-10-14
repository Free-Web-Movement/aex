//! TLS SNI/ALPN routing demo.
//!
//! Registers [`TlsDetector`] on a Server; every TLS connection is
//! routed by its ClientHello names, everything else falls back to the plain
//! TCP handler.
//!
//! Run: `cargo run --example detector_tls`
//! Probe: `printf '\x16\x03\x01...' | nc 127.0.0.1 9443` or any TLS client.

#[path = "../detectors_common/mod.rs"]
mod detectors_common;

use std::net::SocketAddr;
use std::sync::Arc;

use detectors_common::{TlsClientHello, TlsDetector};
use tokio::io::AsyncWriteExt;

use aex::connection::context::Context;
use aex::connection::global::GlobalContext;
use aex::detect::DetectionState;
use aex::protocols::tcp::{self, Tcp};
use aex::server::Server;

async fn sni_echo(mut ctx: Context) {
    let hello = ctx
        .local
        .get_ref::<DetectionState>()
        .and_then(|s| s.get_scratch::<TlsClientHello>())
        .unwrap_or_default();
    if let Some(w) = ctx.writer.as_mut() {
        let _ = w
            .write_all(format!("tls hello sni={:?} alpn={:?}\n", hello.sni, hello.alpn).as_bytes())
            .await;
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let addr: SocketAddr = "127.0.0.1:9443".parse()?;
    let server = Server::new(Arc::new(GlobalContext::new(addr, None)))
        .detector(Arc::new(TlsDetector))
        .push(tcp::from_handler(
            "tls",
            Some(Arc::new(|ctx: Context| tokio::spawn(sni_echo(ctx)))),
        ))
        .push(
            Tcp::new(Some(Arc::new(|ctx: Context| {
                tokio::spawn(async move {
                    let mut ctx = ctx;
                    // Non-TLS traffic lands here.
                    if let Some(w) = ctx.writer.as_mut() {
                        let _ = w.write_all(b"plain tcp\n").await;
                    }
                })
            })))
            .shared(),
        );

    eprintln!("detector_tls listening on {addr}");
    server.start(addr).await
}
