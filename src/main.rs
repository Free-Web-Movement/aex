use std::net::SocketAddr;
use std::sync::Arc;

use clap::Parser;
use futures::future::FutureExt;

use aex::{
    get,
    handler::{ Executor, HTTPContext },
    protocol::{ header::HeaderKey, status::StatusCode },
    route,
    server::HTTPServer,
    trie::{ NodeType, TrieNode }, // 👈 关键：TrieRouter
};

#[derive(Parser, Debug)]
#[command(name = "aex")]
struct Opt {
    #[arg(long, default_value = "0.0.0.0")]
    ip: String,

    #[arg(long, default_value_t = 9000)]
    port: u16,
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> anyhow::Result<()> {
    let opt = Opt::parse();

    let addr: SocketAddr = format!("{}:{}", opt.ip, opt.port).parse()?;

    // 1️⃣ 构建 TrieRouter
    let mut route = TrieNode::new(NodeType::Static("root".into()));

    route!(
        route,
        get!("/", |ctx: &mut HTTPContext| {
            (
                async move {
                    // ctx.res.status = StatusCode::Ok;
                    // ctx.res.headers.insert(HeaderKey::ContentType, "text/plain".into());

                    ctx.res.body.push("Hello world!".to_string());

                    // false = 不继续 middleware（如果你还保留这个语义）
                    true
                }
            ).boxed()
        })
    );

    // 2️⃣ 启动 HTTPServer（直接吃 trie）
    let server = HTTPServer::new(addr, route);

    server.run().await?;
    Ok(())
}
