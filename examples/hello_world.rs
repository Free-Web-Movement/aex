use aex::connection::context::Context;
use aex::http::router::Router as HttpRouter;
use aex::protocols::http1::Http1;
use aex::protocols::tcp::Tcp;
use aex::server::Server;
use std::net::SocketAddr;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let addr: SocketAddr = "0.0.0.0:8080".parse()?;
    let mut router = HttpRouter::default();

    router.get("/", |_: &mut Context| "Hello world!");

    Server::default()
        .push(
            Tcp::new(None)
                .sub(Http1::new(Some(Arc::new(router)), None).shared())
                .shared(),
        )
        .start(addr)
        .await?;
    Ok(())
}
