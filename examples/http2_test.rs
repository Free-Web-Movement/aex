use aex::http::router::{NodeType, Router as HttpRouter};
use aex::server::Server;
use aex::exe;
use std::net::SocketAddr;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let addr: SocketAddr = "0.0.0.0:8080".parse()?;
    let mut router = HttpRouter::new(NodeType::Static("root".into()));

    router.get("/", exe!(|ctx| {
        ctx.send("Hello from HTTP server!", None);
        true
    })).register();

    router.get("/api/users", exe!(|ctx| {
        ctx.send(r#"{"users": ["alice", "bob"]}"#, None);
        true
    })).register();

    println!("Starting HTTP server on {}", addr);

    Server::new(addr, None)
        .http(router)
        .start()
        .await?;
    Ok(())
}
