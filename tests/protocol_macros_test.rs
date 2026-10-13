use aex::connection::context::Context;
use aex::http::router::Router as HttpRouter;
use aex::unified::Server;
use aex::{http, http1, http2, tcp, udp, ws};
use std::net::SocketAddr;
use tokio::time::{Duration, sleep};

async fn free_addr() -> SocketAddr {
    let probe = tokio::net::TcpListener::bind("[::1]:0").await.unwrap();
    let addr = probe.local_addr().unwrap();
    drop(probe);
    addr
}

#[tokio::test]
async fn nested_macros_serve_http() {
    let mut router = HttpRouter::default();
    router.get("/", |_ctx: &mut Context| true);

    let addr = free_addr().await;
    let server = Server::solo(addr)
        .push(tcp!(http!(router), http2!()))
        .push(udp!());

    tokio::spawn(async move {
        let _ = server.start().await;
    });
    sleep(Duration::from_millis(300)).await;

    let res = reqwest::get(format!("http://{}", addr)).await.unwrap();
    assert_eq!(res.status(), 200);
}

#[tokio::test]
async fn direct_http_macro_serves_http() {
    let mut router = HttpRouter::default();
    router.get("/", |_ctx: &mut Context| true);

    let addr = free_addr().await;
    let server = Server::solo(addr).push(http!(router));

    tokio::spawn(async move {
        let _ = server.start().await;
    });
    sleep(Duration::from_millis(300)).await;

    let res = reqwest::get(format!("http://{}", addr)).await.unwrap();
    assert_eq!(res.status(), 200);
}

#[tokio::test]
async fn http1_alias_serves_http() {
    let mut router = HttpRouter::default();
    router.get("/", |_ctx: &mut Context| true);

    let addr = free_addr().await;
    let server = Server::solo(addr).push(tcp!(http1!(router)));

    tokio::spawn(async move {
        let _ = server.start().await;
    });
    sleep(Duration::from_millis(300)).await;

    let res = reqwest::get(format!("http://{}", addr)).await.unwrap();
    assert_eq!(res.status(), 200);
}

#[tokio::test]
async fn explicit_websocket_passed_into_http() {
    let mut router = HttpRouter::default();
    router.get("/", |_ctx: &mut Context| true);

    let ws_router = HttpRouter::default();

    let addr = free_addr().await;
    let server = Server::solo(addr).push(http!(router, ws!(ws_router)));

    tokio::spawn(async move {
        let _ = server.start().await;
    });
    sleep(Duration::from_millis(300)).await;

    let res = reqwest::get(format!("http://{}", addr)).await.unwrap();
    assert_eq!(res.status(), 200);
}
