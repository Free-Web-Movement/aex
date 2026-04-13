use aex::http::router::{NodeType, Router as HttpRouter};
use aex::server::HTTPServer;
use aex::tcp::types::{Command, RawCodec};
use aex::exe;
use std::net::SocketAddr;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let addr: SocketAddr = "0.0.0.0:8080".parse()?;
    let mut router = HttpRouter::new(NodeType::Static("root".into()));

    router.get("/", exe!(|ctx| {
        ctx.send("Hello from HTTP/2!", None);
        true
    })).register();

    router.get("/api/users", exe!(|ctx| {
        ctx.send(r#"{"users": ["alice", "bob"]}"#, None);
        true
    })).register();

    println!("Starting HTTP/2 server on {}", addr);
    println!("Use: curl --http2 http://localhost:8080/");

    HTTPServer::new(addr, None)
        .http(router)
        .http2()
        .start::<RawCodec, RawCodec>(Arc::new(|c| c.id()))
        .await?;
    Ok(())
}