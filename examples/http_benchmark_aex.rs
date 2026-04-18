//! HTTP Server Benchmark - AEX
//!
//! Tests: no URL, static URL, dynamic URL

use aex::http::router::Router as HttpRouter;
use aex::server::Server;
use aex::exe;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::time;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr: SocketAddr = "127.0.0.1:8080".parse()?;

    let mut http_router = HttpRouter::default();
    
    // 1. No URL (root)
    http_router.get("/", exe!(|ctx| {
        ctx.send("Hello", None);
        true
    })).register();
    
    // 2. Static URL
    http_router.get("/api/users", exe!(|ctx| {
        ctx.send(r#"[{"id":1,"name":"alice"}]"#, None);
        true
    })).register();
    
    // 3. Dynamic URL
    http_router.get("/api/users/:id", exe!(|ctx| {
        ctx.send(r#"{"id":1}"#, None);
        true
    })).register();

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