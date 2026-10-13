use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use aex::connection::context::Context;
use aex::connection::global::GlobalContext;
use aex::protocols::classify::ProtocolKind;
use aex::protocols::http1::Http11Detector;
use aex::protocols::http2::Http2Detector;
use aex::protocols::tcp::from_handler;
use aex::unified::{
    DetectionState, DetectorRegistry, Position, ProtocolDetector, UnifiedServer, Verdict,
};
use aex::{http2, tcp_raw, udp};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{Duration, sleep};

async fn free_addr() -> SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    drop(l);
    addr
}

#[test]
fn test_protocol_detect_remaining_http_methods() {
    for m in [
        b"PUT ".as_slice(),
        b"DELETE ".as_slice(),
        b"PATCH ".as_slice(),
        b"HEAD ".as_slice(),
        b"OPTIONS ".as_slice(),
        b"CONNECT ".as_slice(),
        b"TRACE ".as_slice(),
    ] {
        assert_eq!(
            ProtocolKind::detect(m, false),
            ProtocolKind::Http11,
            "method {m:?}"
        );
    }
}

#[test]
fn test_protocol_detect_edge_cases() {
    assert_eq!(ProtocolKind::detect(b"GET", false), ProtocolKind::TCP);
    assert_eq!(
        ProtocolKind::detect(b"get / HTTP/1.1", false),
        ProtocolKind::TCP
    );
    assert_eq!(
        ProtocolKind::detect(b"GET / HTTP/1.1", true),
        ProtocolKind::UDP
    );
    assert_eq!(
        ProtocolKind::detect(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n", false),
        ProtocolKind::Http11
    );
}

#[test]
fn test_unified_server_new_defaults_and_clone() {
    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let globals = Arc::new(GlobalContext::new(addr, None));

    let s = UnifiedServer::at(addr, globals);
    assert!(s.detect_enabled);
    assert!(s.protocols.is_empty());
    assert!(s.datagram.is_none());
    assert!(s.registry.is_empty());

    let c = s.clone();
    assert_eq!(c.addr, addr);
    assert!(c.registry.is_empty());
    assert!(c.protocols.is_empty());
}

#[test]
fn test_unified_builder_methods() {
    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let globals = Arc::new(GlobalContext::new(addr, None));

    let registry = DetectorRegistry::new();
    registry.register(Arc::new(Http11Detector)).unwrap();
    let shared = Arc::new(registry);

    let server = UnifiedServer::at(addr, globals)
        .with_registry(shared.clone())
        .detector_at(Position::Front, Arc::new(Http2Detector))
        .push(from_handler(
            "extra",
            Some(Arc::new(|_ctx: Context| tokio::spawn(async {}))),
        ))
        .push(udp!())
        .detection(false);

    assert!(!server.detect_enabled);
    assert!(server.datagram.is_some());
    assert!(server.protocols.iter().any(|p| p.name() == "extra"));
    assert_eq!(server.registry.list(), vec!["http2", "http11"]);
}

struct Claimer;
impl ProtocolDetector for Claimer {
    fn name(&self) -> &str {
        "claimer"
    }
    fn protocol(&self) -> &str {
        "claimed-proto"
    }
    fn detect(&self, buf: &[u8], _state: &mut DetectionState) -> Verdict {
        if buf.starts_with(b"CLM ") {
            Verdict::Match
        } else {
            Verdict::NeedMore(4)
        }
    }
}

#[tokio::test]
async fn test_detection_disabled_skips_peek() {
    let addr = free_addr().await;
    let hits = Arc::new(AtomicUsize::new(0));
    let h = hits.clone();

    let server = UnifiedServer::at(addr, Arc::new(GlobalContext::new(addr, None)))
        .detector(Arc::new(Http11Detector))
        .detection(false)
        .push(tcp_raw!(move |_ctx| {
            let h = h.clone();
            tokio::spawn(async move {
                h.fetch_add(1, Ordering::SeqCst);
            })
        }));

    tokio::spawn(async move {
        server.start().await.ok();
    });
    sleep(Duration::from_millis(100)).await;

    let mut conn = TcpStream::connect(addr).await.unwrap();
    conn.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n")
        .await
        .unwrap();
    conn.shutdown().await.ok();
    sleep(Duration::from_millis(150)).await;

    assert_eq!(hits.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_claim_without_custom_handler_falls_back_to_tcp() {
    let addr = free_addr().await;
    let hits = Arc::new(AtomicUsize::new(0));
    let h = hits.clone();

    let server = UnifiedServer::at(addr, Arc::new(GlobalContext::new(addr, None)))
        .detector(Arc::new(Claimer))
        .push(tcp_raw!(move |_ctx| {
            let h = h.clone();
            tokio::spawn(async move {
                h.fetch_add(1, Ordering::SeqCst);
            })
        }));

    tokio::spawn(async move {
        server.start().await.ok();
    });
    sleep(Duration::from_millis(100)).await;

    let mut conn = TcpStream::connect(addr).await.unwrap();
    conn.write_all(b"CLM payload").await.unwrap();
    conn.shutdown().await.ok();
    sleep(Duration::from_millis(200)).await;

    assert_eq!(hits.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_http2_preface_without_enable_falls_back_to_tcp() {
    let addr = free_addr().await;
    let hits = Arc::new(AtomicUsize::new(0));
    let h = hits.clone();

    let server = UnifiedServer::at(addr, Arc::new(GlobalContext::new(addr, None)))
        .detector(Arc::new(Http2Detector))
        .push(tcp_raw!(move |_ctx| {
            let h = h.clone();
            tokio::spawn(async move {
                h.fetch_add(1, Ordering::SeqCst);
            })
        }));

    tokio::spawn(async move {
        server.start().await.ok();
    });
    sleep(Duration::from_millis(100)).await;

    let mut conn = TcpStream::connect(addr).await.unwrap();
    conn.write_all(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n")
        .await
        .unwrap();
    conn.shutdown().await.ok();
    sleep(Duration::from_millis(200)).await;

    assert_eq!(hits.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_claim_with_no_handlers_drops_without_panic() {
    let addr = free_addr().await;

    let server = UnifiedServer::at(addr, Arc::new(GlobalContext::new(addr, None)))
        .detector(Arc::new(Claimer));

    tokio::spawn(async move {
        server.start().await.ok();
    });
    sleep(Duration::from_millis(100)).await;

    let mut conn = TcpStream::connect(addr).await.unwrap();
    conn.write_all(b"CLM payload").await.unwrap();
    conn.shutdown().await.ok();
    sleep(Duration::from_millis(100)).await;

    let mut conn2 = TcpStream::connect(addr).await.unwrap();
    conn2.write_all(b"CLM again").await.unwrap();
    conn2.shutdown().await.ok();
    sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_tcp_connection_with_no_handler_drops() {
    let addr = free_addr().await;

    let server = UnifiedServer::at(addr, Arc::new(GlobalContext::new(addr, None)));
    tokio::spawn(async move {
        server.start().await.ok();
    });
    sleep(Duration::from_millis(100)).await;

    let mut conn = TcpStream::connect(addr).await.unwrap();
    conn.write_all(b"anything").await.unwrap();
    conn.shutdown().await.ok();
    sleep(Duration::from_millis(100)).await;

    TcpStream::connect(addr).await.unwrap();
}

#[tokio::test]
async fn test_http_request_with_no_handler_returns_error() {
    let addr = free_addr().await;

    let server = UnifiedServer::at(addr, Arc::new(GlobalContext::new(addr, None)))
        .push(aex::http!());

    tokio::spawn(async move {
        server.start().await.ok();
    });
    sleep(Duration::from_millis(100)).await;

    let mut conn = TcpStream::connect(addr).await.unwrap();
    conn.write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = [0u8; 64];
    let n = conn.read(&mut buf).await.unwrap();
    let head = String::from_utf8_lossy(&buf[..n]);
    assert!(head.starts_with("HTTP/1.1 400"), "got {head}");
}

#[tokio::test]
async fn test_udp_handler_receives_datagram() {
    let addr = free_addr().await;
    let hits = Arc::new(AtomicUsize::new(0));
    let h = hits.clone();

    let server = UnifiedServer::at(addr, Arc::new(GlobalContext::new(addr, None))).push(udp!(
        move |ctx| {
            let h = h.clone();
            tokio::spawn(async move {
                let data: Vec<u8> = ctx.get().unwrap_or_default();
                if data == b"ping" {
                    h.fetch_add(1, Ordering::SeqCst);
                }
            })
        }
    ));

    tokio::spawn(async move {
        server.start().await.ok();
    });
    sleep(Duration::from_millis(100)).await;

    let sock = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    sock.send_to(b"ping", addr).await.unwrap();
    sleep(Duration::from_millis(200)).await;

    assert_eq!(hits.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_http2_handler_receives_request() {
    use h2::client;

    let addr = free_addr().await;
    let hits = Arc::new(AtomicUsize::new(0));
    let h = hits.clone();

    let server = UnifiedServer::at(addr, Arc::new(GlobalContext::new(addr, None))).push(http2!(
        move |ctx: &mut Context| {
            let h = h.clone();
            let meta = ctx.local.get_ref::<aex::http::meta::HttpMetadata>();
            let _path = meta.map(|m| m.path.clone()).unwrap_or_default();
            Box::pin(async move {
                h.fetch_add(1, Ordering::SeqCst);
                true
            })
        }
    ));

    tokio::spawn(async move {
        server.start().await.ok();
    });
    sleep(Duration::from_millis(100)).await;

    // h2 client 发送真实请求
    let client_stream = TcpStream::connect(addr).await.unwrap();
    let (mut send_request, mut conn) = client::handshake(client_stream).await.unwrap();
    tokio::spawn(async move {
        let _ = conn.await;
    });

    let request = http::Request::builder()
        .method("GET")
        .uri("http://localhost/path")
        .body(())
        .unwrap();
    let (response, _) = send_request.send_request(request, false).unwrap();
    // 不强制等待完整响应（handler 返回 true 但响应写回可能因空 body 结束）
    drop(response);

    // handler 应被调用
    tokio::time::timeout(Duration::from_secs(2), async {
        while hits.load(Ordering::SeqCst) == 0 {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("http2 handler never called");
    assert_eq!(hits.load(Ordering::SeqCst), 1);
}
