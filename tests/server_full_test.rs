use aex::connection::context::{get_tcp_router, TypeMapExt};
use aex::http::middlewares::websocket::WebSocket;
use aex::http::router::Router as HttpRouter;
use aex::server::{HttpVersions, Server};
use aex::tcp::router::Router as TcpRouter;
use aex::tcp::types::{Codec, RawCodec};
use aex::udp::router::Router as UdpRouter;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::time::Duration;

fn addr() -> SocketAddr {
    "127.0.0.1:0".parse().unwrap()
}
async fn free_addr() -> SocketAddr {
    let t = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let a = t.local_addr().unwrap();
    drop(t);
    a
}

#[test]
fn http_versions_v1() {
    let v = HttpVersions::v1();
    assert!(!v.has_http2());
    assert!(!v.has_http3());
}
#[test]
fn http_versions_v1_v2() {
    let v = HttpVersions::v1_v2();
    assert!(v.has_http2());
    assert!(!v.has_http3());
}
#[test]
fn http_versions_v1_v2_v3() {
    let v = HttpVersions::v1_v2_v3();
    assert!(v.has_http2());
    assert!(v.has_http3());
}
#[test]
fn http_versions_default_is_empty() {
    let v = HttpVersions::default();
    assert!(!v.has_http2());
    assert!(!v.has_http3());
}

#[test]
fn server_new_defaults() {
    let a = addr();
    let server = Server::new(a, None);
    assert_eq!(server.addr, a);
    assert!(!server.has_ws());
    assert!(server.globals.routers.get_value::<Arc<HttpRouter>>().is_none());
}

#[test]
fn server_ws_sets_handler() {
    let server = Server::new(addr(), None).ws(WebSocket::new());
    assert!(server.has_ws());
}

#[test]
fn server_http_sets_router() {
    let mut router = HttpRouter::default();
    router.get("/", |_ctx: &mut aex::connection::context::Context| true);
    let server = Server::new(addr(), None).http(router);
    assert!(server.globals.routers.get_value::<Arc<HttpRouter>>().is_some());
}

#[test]
fn server_http2_without_router_keeps_codec_unset() {
    let server = Server::new(addr(), None).http2();
    assert!(server.globals.h2_codec.get().is_none());
}

#[test]
fn server_http2_after_http_sets_codec() {
    let server = Server::new(addr(), None)
        .http(HttpRouter::default())
        .http2();
    assert!(server.globals.h2_codec.get().is_some());
}

#[test]
fn server_tcp_stores_router() {
    let server = Server::new(addr(), None).tcp(TcpRouter::<RawCodec, RawCodec>::new());
    assert!(get_tcp_router::<RawCodec, RawCodec>(&server.globals.routers).is_some());
}

#[test]
fn server_udp_stores_router() {
    use aex::connection::context::get_udp_router;
    let server = Server::new(addr(), None).udp(UdpRouter::<RawCodec, RawCodec>::new());
    assert!(get_udp_router::<RawCodec, RawCodec>(&server.globals.routers).is_some());
}

#[tokio::test]
async fn server_start_udp_missing_router_errors() {
    let a = free_addr().await;
    let server = Server::new(a, None);
    let token = tokio_util::sync::CancellationToken::new();
    let res = server.start_udp::<RawCodec, RawCodec>(token).await;
    assert!(res.is_err());
}

#[tokio::test]
async fn server_start_http_only_serves_route() {
    let a = free_addr().await;
    let mut router = HttpRouter::default();
    router.get("/ping", |_ctx: &mut aex::connection::context::Context| "pong");
    let server = Server::new(a, None).http(router);
    let handle = tokio::spawn(async move { server.start().await });

    tokio::time::sleep(Duration::from_millis(200)).await;
    let resp = reqwest::get(format!("http://{}/ping", a)).await.unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.unwrap(), "pong");
    handle.abort();
}

#[tokio::test]
async fn server_start_tcp_accepts_connection() {
    use tokio::io::AsyncWriteExt;
    let a = free_addr().await;
    let server = Server::new(a, None);
    let token = tokio_util::sync::CancellationToken::new();
    let token2 = token.clone();
    let handle = tokio::spawn(async move {
        let _ = server.start_tcp::<RawCodec, RawCodec>(token2).await;
    });

    tokio::time::sleep(Duration::from_millis(200)).await;
    let mut conn = tokio::net::TcpStream::connect(a).await.unwrap();
    conn.write_all(b"\x00PING").await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    token.cancel();
    let _ = handle.await;
}

#[tokio::test]
async fn server_start_udp_dispatches_to_router() {
    let a = free_addr().await;
    let counter = Arc::new(AtomicUsize::new(0));
    let c = counter.clone();
    let mut udp = UdpRouter::<RawCodec, RawCodec>::new()
        .extractor(|c: &RawCodec| u32::from_le_bytes(c.0[..4].try_into().unwrap()));
    udp.on(42, move |_g, _f, _c, _a, _s| {
        let c = c.clone();
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            Ok::<bool, anyhow::Error>(true)
        }
    });

    let server = Server::new(a, None).udp(udp);
    let token = tokio_util::sync::CancellationToken::new();
    let handle = tokio::spawn(async move {
        let _ = server.start_udp::<RawCodec, RawCodec>(token).await;
    });

    tokio::time::sleep(Duration::from_millis(200)).await;
    let client = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let data = RawCodec(42u32.to_le_bytes().to_vec()).encode().unwrap();
    client.send_to(&data, a).await.unwrap();

    tokio::time::timeout(Duration::from_secs(2), async {
        while counter.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("udp handler never ran");
    assert_eq!(counter.load(Ordering::SeqCst), 1);
    handle.abort();
}

#[tokio::test]
async fn server_start_multi_protocol_with_tcp_and_udp() {
    use aex::connection::context::get_udp_router;
    let a = free_addr().await;
    let counter = Arc::new(AtomicUsize::new(0));
    let c = counter.clone();

    let mut udp = UdpRouter::<RawCodec, RawCodec>::new()
        .extractor(|c: &RawCodec| u32::from_le_bytes(c.0[..4].try_into().unwrap()));
    udp.on(99, move |_g, _f, _c, _a, _s| {
        let c = c.clone();
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            Ok::<bool, anyhow::Error>(true)
        }
    });

    // tcp + udp 同时配置，start() 走 start_multi_protocol
    let server = Server::new(a, None)
        .tcp(TcpRouter::<RawCodec, RawCodec>::new())
        .udp(udp);
    assert!(get_udp_router::<RawCodec, RawCodec>(&server.globals.routers).is_some());

    let handle = tokio::spawn(async move {
        let _ = server.start().await;
    });

    tokio::time::sleep(Duration::from_millis(300)).await;

    // UDP 应可收到数据
    let client = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let data = RawCodec(99u32.to_le_bytes().to_vec()).encode().unwrap();
    client.send_to(&data, a).await.unwrap();

    tokio::time::timeout(Duration::from_secs(2), async {
        while counter.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("udp handler never ran via start()");
    assert_eq!(counter.load(Ordering::SeqCst), 1);
    handle.abort();
}
