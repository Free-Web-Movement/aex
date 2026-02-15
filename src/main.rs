use std::{ net::SocketAddr, sync::Arc };

use clap::Parser;

use aex::{
    get, middlewares::websocket::WebSocket, route, router::{ NodeType, Router }, server::HTTPServer, types::{ BinaryHandler, HTTPContext, TextHandler }
};
use futures::FutureExt;

pub async fn start_ws_main() {
    let text_handler: TextHandler = Arc::new(|ws: &WebSocket, ctx: &mut HTTPContext, text: String| {
        (
            async move {
                // processing here
                true
            }
        ).boxed()
    });

    let binary_handler: BinaryHandler = Arc::new(
        |ws: &WebSocket, ctx: &mut HTTPContext, data: Vec<u8>| {
            (
                async move {
                    // processing here
                    true
                }
            ).boxed()
        }
    );

    let ws = WebSocket {
        on_binary: Some(binary_handler),
        on_text: Some(text_handler),
    };

    let ws_mw = WebSocket::to_middleware(ws);

    let ws_params = get!(
        "/",
        |ctx|
            (
                async move {
                    // ctx.res.body.push("Hello world!".to_string());
                    true
                }
            ).boxed(),
        [ws_mw]
    );

    let mut route = Router::new(NodeType::Static("root".into()));

    route!(route, ws_params);
}

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
    let mut route = Router::new(NodeType::Static("root".into()));

    route!(
        route,
        get!("/", |ctx: &mut HTTPContext| {
            Box::pin(async move {
                ctx.res.body.push("Hello world!".to_string());
                true
            }).boxed()
        })
    );

    // route.insert(
    //     "/",
    //     Some("GET"),
    //     Arc::new(|ctx: &mut HTTPContext| {
    //         Box::pin(async move {
    //             ctx.res.body.push("Hello world!".to_string());
    //             true
    //         }).boxed()
    //     }),
    //     None // 传入 WebSocket 中间件
    // );

    // 2️⃣ 启动 HTTPServer（直接吃 trie）
    let server = HTTPServer::new(addr, route);

    server.run().await?;
    Ok(())
}
