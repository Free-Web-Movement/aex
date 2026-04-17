use aex::http::router::Router as HttpRouter;
use aex::server::Server;
use aex::exe;
use std::net::SocketAddr;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let addr: SocketAddr = "0.0.0.0:8080".parse()?;

    let mut http_router = HttpRouter::default();

    http_router.get("/", exe!(|ctx| {
        ctx.send("Hello world!", None);
        true
    })).register();

    Server::new(addr, None)
        .http(http_router)
        .start()
        .await?;

    Ok(())
}
