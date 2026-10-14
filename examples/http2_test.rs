use aex::connection::context::Context;
use aex::http::router::Router as HttpRouter;
use aex::protocols::http1::Http1;
use aex::protocols::http2::Http2;
use aex::protocols::tcp::Tcp;
use aex::server::Server;
use std::net::SocketAddr;
use std::sync::Arc;

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

    Server::default()
        .push(
            Tcp::new(None)
                .sub(Http1::new(Some(Arc::new(router)), None).shared())
                .sub(Http2::new(None, None).shared())
                .shared(),
        )
        .start(addr)
        .await?;
    Ok(())
}
