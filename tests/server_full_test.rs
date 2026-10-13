use aex::connection::context::TypeMapExt;
use aex::http::router::Router as HttpRouter;
use aex::tcp::types::{Codec, RawCodec};
use aex::udp::router::Router as UdpRouter;
use aex::unified::Server;
use aex::{http, tcp};
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

fn http_server(addr: SocketAddr, router: HttpRouter) -> Server {
    Server::solo(addr).push(tcp!(http!(router)))
}

#[test]
fn server_new_defaults() {
    let a = addr();
    let server = Server::solo(a);
    assert_eq!(server.addr, a);
    assert!(
        server
            .globals
            .routers
            .get_value::<Arc<HttpRouter>>()
            .is_none()
    );
}

#[tokio::test]
async fn server_start_http_only_serves_route() {
    let a = free_addr().await;
    let mut router = HttpRouter::default();
    router.get(
        "/ping",
        |_ctx: &mut aex::connection::context::Context| "pong",
    );
    let server = http_server(a, router);
    let handle = tokio::spawn(async move { server.start().await });

    tokio::time::sleep(Duration::from_millis(200)).await;
    let resp = reqwest::get(format!("http://{}/ping", a)).await.unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.unwrap(), "pong");
    handle.abort();
}

#[tokio::test]
async fn server_start_accepts_connection() {
    use tokio::io::AsyncWriteExt;
    let a = free_addr().await;
    let server = Server::solo(a).push(tcp!());
    let handle = tokio::spawn(async move {
        let _ = server.start().await;
    });

    tokio::time::sleep(Duration::from_millis(200)).await;
    let mut conn = tokio::net::TcpStream::connect(a).await.unwrap();
    conn.write_all(b"\x00PING").await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    handle.abort();
}

#[tokio::test]
async fn udp_router_dispatches_datagram() {
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

    let server = Server::solo(a);
    let globals = server.globals.clone();
    let udp = Arc::new(udp);
    let sock = Arc::new(tokio::net::UdpSocket::bind(a).await.unwrap());
    let handle = tokio::spawn(async move {
        let _ = udp.handle(globals, sock).await;
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

    // TCP 由统一 server 启动，UDP 由 router 在后台独立收包。
    let server = Server::solo(a).push(tcp!());
    let globals = server.globals.clone();
    let tcp_handle = tokio::spawn(async move {
        let _ = server.start().await;
    });

    let udp = Arc::new(udp);
    let sock = Arc::new(tokio::net::UdpSocket::bind(a).await.unwrap());
    let udp_handle = tokio::spawn(async move {
        let _ = udp.handle(globals, sock).await;
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
    tcp_handle.abort();
    udp_handle.abort();
}
