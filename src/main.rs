// src/main.rs
use std::{collections::HashMap, net::SocketAddr};
use std::sync::Arc;

use clap::Parser;
use futures::future::FutureExt;
use tokio::sync::Mutex;

use aex::{
    handler::{ Executor, HTTPContext }, protocol::status::StatusCode, res::Response, router::Router, server::HTTPServer
};

#[derive(Parser, Debug)]
#[command(name = "aex")]
struct Opt {
    #[arg(long, default_value = "0.0.0.0")]
    ip: String,

    #[arg(long, default_value_t = 9000)]
    port: u16,
}

/// Hello world executor
fn hello_world_executor() -> Executor {
    Arc::new(|ctx: Arc<Mutex<HTTPContext>>| {
        (
            async move {
                let res = {
                    let ctx_guard = ctx.lock().await;
                    ctx_guard.res.clone()
                };

                let mut res = res.lock().await;
                let headers = HashMap::<String, String>::new();
                let _ = Response::send_bytes(&mut res.writer, StatusCode::Ok, headers, b"Hello world!").await;

                // false = 终止 middleware 链
                false
            }
        ).boxed()
    })
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> anyhow::Result<()> {
    let opt = Opt::parse();

    let addr: SocketAddr = format!("{}:{}", opt.ip, opt.port).parse()?;

    // 1️⃣ 构建 Router（完全 mut-less 使用）
    let mut router = Router::new();
    router.get(vec!["/"], vec![hello_world_executor()]);

    // 2️⃣ 启动 HTTPServer
    let server = HTTPServer::new(addr, router);

    server.run().await?;
    Ok(())
}
