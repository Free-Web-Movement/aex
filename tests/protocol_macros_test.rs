use aex::connection::context::Context;
use aex::http::router::Router as HttpRouter;
use aex::{http, http1, http2, tcp, udp, ws};
use aex::{protocol::Protocol, server::Server};
use std::net::SocketAddr;
use std::sync::Arc;
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
    let server = Server::default()
        .push(tcp!(http1!(router), http2!()))
        .push(udp!());

    tokio::spawn(async move {
        let _ = server.start(addr).await;
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
    let server = Server::default().push(http!(router));

    tokio::spawn(async move {
        let _ = server.start(addr).await;
    });
    sleep(Duration::from_millis(300)).await;

    let res = reqwest::get(format!("http://{}", addr)).await.unwrap();
    assert_eq!(res.status(), 200);
}

#[test]
fn http_macro_shares_one_router_between_http1_and_http2() {
    let mut router = HttpRouter::default();
    router.get("/", |_ctx: &mut Context| true);

    let tcp = http!(router); // Arc<Tcp>
    let subs = tcp.sub_handlers();
    let h1 = subs.iter().find(|p| p.name() == "http").expect("http1");
    let h2 = subs.iter().find(|p| p.name() == "http2").expect("http2");
    let r1 = h1.router().expect("http1 router");
    let r2 = h2.router().expect("http2 router");
    assert!(
        Arc::ptr_eq(&r1, &r2),
        "http1 and http2 must share the same router"
    );
}

#[tokio::test]
async fn http1_alias_serves_http() {
    let mut router = HttpRouter::default();
    router.get("/", |_ctx: &mut Context| true);

    let addr = free_addr().await;
    let server = Server::default().push(http!(router));

    tokio::spawn(async move {
        let _ = server.start(addr).await;
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
    let server = Server::default().push(http!(router, ws!(ws_router)));

    tokio::spawn(async move {
        let _ = server.start(addr).await;
    });
    sleep(Duration::from_millis(300)).await;

    let res = reqwest::get(format!("http://{}", addr)).await.unwrap();
    assert_eq!(res.status(), 200);
}
