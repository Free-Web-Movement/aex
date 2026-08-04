//! HTTP Server Benchmark - AEX
//!
//! Tests: no URL, static URL, dynamic URL

use aex::connection::context::Context;
use aex::http::router::Router as HttpRouter;
use aex::server::Server;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::time;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr: SocketAddr = "127.0.0.1:8090".parse()?;

    let mut http_router = HttpRouter::default();

    // 1. No URL (root)
    http_router.get("/", |ctx: &mut Context| {
        ctx.send("Hello", None);
        true
    });

    // 2. Static URL
    http_router.get("/api/users", |ctx: &mut Context| {
        ctx.send(r#"[{"id":1,"name":"alice"}]"#, None);
        true
    });

    // 3. Dynamic URL
    http_router.get("/api/users/:id", |ctx: &mut Context| {
        ctx.send(r#"{"id":1}"#, None);
        true
    });

    let server = Server::new(addr, None).http(http_router);
    println!("AEX server on {}", addr);

    let handle = tokio::spawn(async move {
        if let Err(e) = server.start().await {
            eprintln!("Server error: {}", e);
        }
    });

    // Block forever
    loop {
        time::sleep(Duration::from_secs(3600)).await;
    }
}
