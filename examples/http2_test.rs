use aex::connection::context::Context;
use aex::http::router::Router as HttpRouter;
use aex::server::Server;
use std::net::SocketAddr;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let addr: SocketAddr = "0.0.0.0:8080".parse()?;
    let mut router = HttpRouter::default();

    router.get("/", |ctx: &mut Context| {
        ctx.send("Hello from HTTP/2 server!", None);
        true
    });

    router.get("/api/users", |ctx: &mut Context| {
        ctx.send(r#"{"users": ["alice", "bob"]}"#, None);
        true
    });

    println!("Starting HTTP/2 server on {}", addr);
    println!("Use: curl --http2 http://localhost:8080/");

    Server::new(addr, None).http(router).http2().start().await?;
    Ok(())
}
