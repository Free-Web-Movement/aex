use aex::connection::context::TypeMapExt;
use aex::http::meta::HttpMetadata;
use aex::http::router::{ NodeType, Router as HttpRouter };
use aex::server::{HTTPServer};
use aex::tcp::types::{Command, RawCodec};
use aex::{ body, exe, get, route };
use std::net::SocketAddr;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let addr: SocketAddr = "0.0.0.0:8080".parse()?;
    let mut router = HttpRouter::new(NodeType::Static("root".into()));

    route!(
        router,
        get!(
            "/",
            exe!(|ctx| {
                body!(ctx, "Hello world!");
                true
            })
        )
    );
    HTTPServer::new(addr, None).http(router).start::<RawCodec,RawCodec>(Arc::new(|c|c.id())).await?;
    Ok(())
}
