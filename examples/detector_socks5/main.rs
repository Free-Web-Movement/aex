//! SOCKS4/4a + SOCKS5 greeting detection demo.
//!
//! Run: `cargo run --example detector_socks5`
//! Probe: `printf '\x05\x01\x00' | nc 127.0.0.1 9080`

#[path = "../detectors_common/mod.rs"]
mod detectors_common;

use std::net::SocketAddr;
use std::sync::Arc;

use detectors_common::{SocksDetector, SocksVersion};
use tokio::io::AsyncWriteExt;

use aex::connection::context::Context;
use aex::connection::global::GlobalContext;
use aex::unified::detect::DetectionState;
use aex::unified::UnifiedServer;

async fn socks_info(ctx: Context) {
    let mut ctx = ctx;
    let version = ctx
        .local
        .get_ref::<DetectionState>()
        .and_then(|s| s.get_scratch::<SocksVersion>());
    let line = match version {
        Some(SocksVersion::V5) => "socks5 greeting received\n".to_string(),
        Some(SocksVersion::V4) => "socks4 request received\n".to_string(),
        None => "unknown\n".to_string(),
    };
    if let Some(w) = ctx.writer.as_mut() {
        let _ = w.write_all(line.as_bytes()).await;
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let addr: SocketAddr = "127.0.0.1:9080".parse()?;
    let server = UnifiedServer::new(addr, Arc::new(GlobalContext::new(addr, None)))
        .detector(Arc::new(SocksDetector))
        .custom_handler("socks", Arc::new(|ctx: Context| tokio::spawn(socks_info(ctx))));

    eprintln!("detector_socks5 listening on {addr}");
    server.start().await
}
