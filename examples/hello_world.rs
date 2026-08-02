use aex::http::router::{NodeType, Router as HttpRouter};
use aex::server::HTTPServer;
use aex::tcp::types::{Command, RawCodec};
use std::net::SocketAddr;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let addr: SocketAddr = "0.0.0.0:8080".parse()?;
    let mut router = HttpRouter::default();

    router.get("/", |_| "Hello world!");

    HTTPServer::new(addr, None).http(router).start().await?;
    Ok(())
}
