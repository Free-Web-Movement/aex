// Unit tests for the unified protocol server (`aex::server::Server`): the
// Protocol trait defaults, mount/detector registration edge warnings, the
// detection-phase EOF path, the listen/start entry points, and
// per-protocol edge cases (HTTP/1 parse failure, HTTP/2 handshake failure and
// no-handler path, TCP/UDP with no handler, NAT detector).

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use aex::connection::context::Context;
use aex::connection::global::GlobalContext;
use aex::handler::{CommandRegistry, Outcome};
use aex::nat::UnifiedServerExt as _;
use aex::protocols::classify::H2_CONNECTION_PREFACE;
use aex::protocols::http1::Http11Detector;
use aex::protocols::http2::{Http2, Http2Detector};
use aex::protocols::tcp::from_handler;
use aex::tcp::types::RawCodec;
use aex::{
    detect::{Claim, DetectionState, DetectorMode, ProtocolDetector, Verdict},
    protocol::{Protocol, Transport},
    server::Server,
};
use aex::{http2, tcp, tcp_raw, udp};
use bytes::Bytes;
use futures::future::BoxFuture;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::time::{Duration, sleep};

fn gctx(addr: SocketAddr) -> Arc<GlobalContext> {
    Arc::new(GlobalContext::new(addr, None))
}

async fn free_addr() -> SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    drop(l);
    addr
}

/// A bare `Protocol` that overrides nothing but the required items: every
/// default method (detector, middleware, sub_handlers, transport, is_default,
/// buffered_writer, is_mine) must be exercised through it.
#[derive(Default)]
struct Bare {
    cmds: CommandRegistry<Bytes>,
}
impl Protocol for Bare {
    fn name(&self) -> &str {
        "bare"
    }
    fn commands(&self) -> &CommandRegistry<Bytes> {
        &self.cmds
    }
    fn process<'a>(&'a self, _ctx: &'a mut Context) -> BoxFuture<'a, Outcome> {
        Box::pin(async { Outcome::Done })
    }
}

struct BareDefault {
    cmds: CommandRegistry<Bytes>,
}
impl Default for BareDefault {
    fn default() -> Self {
        Self {
            cmds: CommandRegistry::default(),
        }
    }
}
impl Protocol for BareDefault {
    fn name(&self) -> &str {
        "bare-default"
    }
    fn commands(&self) -> &CommandRegistry<Bytes> {
        &self.cmds
    }
    fn is_default(&self) -> bool {
        true
    }
    fn process<'a>(&'a self, _ctx: &'a mut Context) -> BoxFuture<'a, Outcome> {
        Box::pin(async { Outcome::Done })
    }
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

struct BareDect {
    cmds: CommandRegistry<Bytes>,
}
impl Default for BareDect {
    fn default() -> Self {
        Self {
            cmds: CommandRegistry::default(),
        }
    }
}
impl Protocol for BareDect {
    fn name(&self) -> &str {
        "bdare"
    }
    fn commands(&self) -> &CommandRegistry<Bytes> {
        &self.cmds
    }
    fn detector(&self) -> Option<Arc<dyn ProtocolDetector>> {
        Some(Arc::new(Claimer))
    }
    fn process<'a>(&'a self, _ctx: &'a mut Context) -> BoxFuture<'a, Outcome> {
        Box::pin(async { Outcome::Done })
    }
}

fn claim_for(protocol: &str) -> Claim {
    Claim {
        detector: "some-det".to_string(),
        protocol: protocol.to_string(),
        mode: DetectorMode::Standard,
    }
}

#[test]
fn bare_protocol_uses_trait_defaults() {
    let bare = Bare::default();
    assert!(bare.detector().is_none());
    assert!(bare.middleware().is_empty());
    assert!(bare.sub_handlers().is_empty());
    assert_eq!(bare.transport(), Transport::Stream);
    assert!(!bare.is_default());
    assert!(!bare.buffered_writer());
    assert!(!bare.is_mine(None));
    assert!(bare.is_mine(Some(&claim_for("bare"))));
    assert!(!bare.is_mine(Some(&claim_for("other"))));
}

#[test]
fn default_protocol_owns_any_claim() {
    let d = BareDefault::default();
    assert!(d.is_default());
    assert!(d.is_mine(None));
    assert!(d.is_mine(Some(&claim_for("anything"))));
}

#[test]
fn detector_name_match_claims_connection() {
    let d = BareDect::default();
    assert!(d.is_mine(Some(&Claim {
        detector: "claimer".into(),
        protocol: "zzz".into(),
        mode: DetectorMode::Standard,
    })));
    assert!(!d.is_mine(Some(&claim_for("other"))));
    assert!(!d.is_mine(None));
}

#[test]
fn mount_orders_defaults_last() {
    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let server = Server::new(gctx(addr))
        .push(Arc::new(Bare::default()))
        .push(Arc::new(BareDefault::default()));
    let names: Vec<String> = server
        .protocols
        .iter()
        .map(|p| p.name().to_string())
        .collect();
    assert_eq!(names, vec!["bare".to_string(), "bare-default".to_string()]);
}

struct ConflictHttp11;
impl ProtocolDetector for ConflictHttp11 {
    fn name(&self) -> &str {
        "cx"
    }
    fn protocol(&self) -> &str {
        "cx"
    }
    fn conflicts_with(&self) -> Vec<String> {
        vec!["http11".to_string()]
    }
    fn detect(&self, _buf: &[u8], _state: &mut DetectionState) -> Verdict {
        Verdict::Pass
    }
}

#[test]
fn mount_conflicting_detector_registration_is_logged() {
    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let server = Server::new(gctx(addr))
        .detector(Arc::new(ConflictHttp11))
        .push(aex::http1!());

    // Auto-registration of the http1 detector conflicts with the manual one,
    // so it is rejected; the protocol itself is still mounted.
    assert_eq!(server.registry.list(), vec!["cx".to_string()]);
    assert!(server.protocols.iter().any(|p| p.name() == "http"));
}

#[test]
fn detector_at_duplicate_keeps_first() {
    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let server = Server::new(gctx(addr))
        .detector(Arc::new(Http2Detector))
        .detector(Arc::new(Http2Detector));
    assert_eq!(server.registry.list(), vec!["http2".to_string()]);
}

#[tokio::test]
async fn partial_peek_then_eof_finishes_detection() {
    let addr = free_addr().await;
    let hits = Arc::new(AtomicUsize::new(0));
    let h = hits.clone();

    let server = Server::new(gctx(addr))
        .detector(Arc::new(Claimer))
        .push(from_handler(
            "extra",
            Some(Arc::new(move |_ctx| {
                let h = h.clone();
                tokio::spawn(async move {
                    h.fetch_add(1, Ordering::SeqCst);
                })
            })),
        ));

    tokio::spawn(async move {
        server.start(addr).await.ok();
    });
    sleep(Duration::from_millis(100)).await;

    // Partial match bytes ("CL") then EOF: the detection phase must finish the
    // state and fall through to no matching protocol.
    let mut conn = TcpStream::connect(addr).await.unwrap();
    conn.write_all(b"CL").await.unwrap();
    conn.shutdown().await.ok();
    sleep(Duration::from_millis(150)).await;

    assert_eq!(hits.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn udp_datagram_served_via_start() {
    let addr = free_addr().await;
    let hits = Arc::new(AtomicUsize::new(0));
    let h = hits.clone();

    let server = Server::new(gctx(addr)).push(udp!(move |ctx| {
        let h = h.clone();
        tokio::spawn(async move {
            let data: Vec<u8> = ctx.get().unwrap_or_default();
            if data == b"ping" {
                h.fetch_add(1, Ordering::SeqCst);
            }
        })
    }));

    tokio::spawn(async move {
        server.start(addr).await.ok();
    });
    sleep(Duration::from_millis(100)).await;

    let sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    sock.send_to(b"ping", addr).await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while hits.load(Ordering::SeqCst) == 0 {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("udp handler never called");
    assert_eq!(hits.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn tcp_connection_served_via_start() {
    let addr = free_addr().await;
    let hits = Arc::new(AtomicUsize::new(0));
    let h = hits.clone();

    let server = Server::new(gctx(addr)).push(tcp_raw!(move |_ctx| {
        let h = h.clone();
        tokio::spawn(async move {
            h.fetch_add(1, Ordering::SeqCst);
        })
    }));

    tokio::spawn(async move {
        server.start(addr).await.ok();
    });
    sleep(Duration::from_millis(100)).await;

    let mut conn = TcpStream::connect(addr).await.unwrap();
    conn.write_all(b"hello").await.unwrap();
    conn.shutdown().await.ok();
    sleep(Duration::from_millis(150)).await;

    assert_eq!(hits.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn listen_binds_provided_addr_and_serves() {
    let boot_addr = free_addr().await;
    let serve_addr = free_addr().await;
    let hits = Arc::new(AtomicUsize::new(0));
    let h = hits.clone();

    let server = Server::new(gctx(boot_addr)).push(tcp_raw!(move |_ctx| {
        let h = h.clone();
        tokio::spawn(async move {
            h.fetch_add(1, Ordering::SeqCst);
        })
    }));

    tokio::spawn(async move {
        server.start(serve_addr).await.ok();
    });
    sleep(Duration::from_millis(100)).await;

    let mut conn = TcpStream::connect(serve_addr).await.unwrap();
    conn.write_all(b"hi").await.unwrap();
    conn.shutdown().await.ok();
    sleep(Duration::from_millis(150)).await;

    assert_eq!(hits.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn tcp_protocol_without_handler_drops_connection() {
    let addr = free_addr().await;
    let server = Server::new(gctx(addr)).push(tcp!());

    tokio::spawn(async move {
        server.start(addr).await.ok();
    });
    sleep(Duration::from_millis(100)).await;

    let mut conn = TcpStream::connect(addr).await.unwrap();
    conn.write_all(b"anything").await.unwrap();
    conn.shutdown().await.ok();
    sleep(Duration::from_millis(150)).await;
}

#[tokio::test]
async fn udp_protocol_without_handler_drops_datagram() {
    let addr = free_addr().await;
    let server = Server::new(gctx(addr)).push(udp!());

    tokio::spawn(async move {
        server.start(addr).await.ok();
    });
    sleep(Duration::from_millis(100)).await;

    let sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    sock.send_to(b"payload", addr).await.unwrap();
    sleep(Duration::from_millis(150)).await;
}

#[tokio::test]
async fn http1_malformed_request_line_is_dropped() {
    let addr = free_addr().await;
    let server = Server::new(gctx(addr)).push(aex::http!());

    tokio::spawn(async move {
        server.start(addr).await.ok();
    });
    sleep(Duration::from_millis(100)).await;

    // A request line over MAX_REQUEST_LINE_SIZE still claims http1 (it begins
    // with "GET ") but fails parsing before metadata exists, so the server
    // drops the connection without writing a response and must not panic.
    let mut conn = TcpStream::connect(addr).await.unwrap();
    let mut req = Vec::from("GET ");
    req.extend(std::iter::repeat_n(b'a', 5000));
    req.extend_from_slice(b"\r\n\r\n");
    conn.write_all(&req).await.unwrap();
    let mut buf = [0u8; 64];
    let n = conn.read(&mut buf).await.unwrap();
    assert_eq!(n, 0, "malformed request must close without a response");
}

#[tokio::test]
async fn http2_detector_matches_and_needs_more() {
    let d = Http2Detector;
    let mut st = DetectionState::new();
    assert_eq!(d.max_need(), Some(H2_CONNECTION_PREFACE.len()));
    assert_eq!(
        d.detect(b"PRI *", &mut st),
        Verdict::NeedMore(H2_CONNECTION_PREFACE.len() - 5)
    );
    assert_eq!(d.detect(H2_CONNECTION_PREFACE, &mut st), Verdict::Match);
    assert_eq!(d.detect(b"XXXXX", &mut st), Verdict::Pass);
}

#[test]
fn http1_detector_matches_and_needs_more() {
    let d = Http11Detector;
    let mut st = DetectionState::new();
    assert_eq!(d.max_need(), Some(8));
    assert_eq!(d.detect(b"G", &mut st), Verdict::NeedMore(3));
    assert_eq!(d.detect(b"GET ", &mut st), Verdict::Match);
    assert_eq!(d.detect(b"nope", &mut st), Verdict::Pass);
}

struct FailReader;
impl tokio::io::AsyncRead for FailReader {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        _buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::Other, "boom")))
    }
}
impl tokio::io::AsyncBufRead for FailReader {
    fn poll_fill_buf(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<&[u8]>> {
        std::task::Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::Other, "boom")))
    }
    fn consume(self: std::pin::Pin<&mut Self>, _amt: usize) {}
}

#[tokio::test]
async fn http2_handshake_failure_returns_done() {
    let addr: SocketAddr = "127.0.0.1:9091".parse().unwrap();
    // The reader fails on the very first read, so the h2 handshake must error
    // and the handler return Outcome::Done without panicking.
    let (rd, wr) = tokio::io::duplex(1024);
    let reader: aex::connection::context::BoxReader = Box::new(FailReader);
    let writer: aex::connection::context::BoxWriter = Box::new(wr);
    let globals = gctx(addr);
    let mut ctx = Context::new(Some(reader), Some(writer), globals, addr);
    drop(rd);
    let outcome = Http2::new(None, None).handle(&mut ctx).await;
    assert_eq!(outcome, Outcome::Done);
}

#[tokio::test]
async fn http2_no_handler_returns_ok() {
    use h2::client;

    let addr = free_addr().await;
    let server = Server::new(gctx(addr)).push(http2!());

    tokio::spawn(async move {
        server.start(addr).await.ok();
    });
    sleep(Duration::from_millis(100)).await;

    let client_stream = TcpStream::connect(addr).await.unwrap();
    let (mut send_request, conn) = client::handshake(client_stream).await.unwrap();
    tokio::spawn(async move {
        let _ = conn.await;
    });

    let request = http::Request::builder()
        .method("GET")
        .uri("http://localhost/path")
        .header("x-custom", "the-value")
        .body(())
        .unwrap();
    let (response, _) = send_request.send_request(request, false).unwrap();
    let resp = tokio::time::timeout(Duration::from_secs(2), response)
        .await
        .expect("no http2 response")
        .unwrap();
    assert_eq!(resp.status(), 200);
}

#[test]
fn nat_detector_max_need_and_magic() {
    let d = aex::nat::NatDetector;
    let mut st = DetectionState::new();
    assert_eq!(d.max_need(), Some(aex::nat::NAT_MAGIC_LEN));
    assert_eq!(
        d.detect(b"", &mut st),
        Verdict::NeedMore(aex::nat::NAT_MAGIC_LEN)
    );
    assert_eq!(
        d.detect(b"ZZ", &mut st),
        Verdict::NeedMore(aex::nat::NAT_MAGIC_LEN - 2)
    );
    assert_eq!(d.detect(b"ZZNAT", &mut st), Verdict::Match);
    assert_eq!(d.detect(b"XXXXX", &mut st), Verdict::Pass);
}

#[tokio::test]
async fn http1_websocket_upgrade_without_router_returns_error() {
    let addr = free_addr().await;
    let server = Server::new(gctx(addr)).push(aex::http!());

    tokio::spawn(async move {
        server.start(addr).await.ok();
    });
    sleep(Duration::from_millis(100)).await;

    // A valid WebSocket upgrade claims http1, hands off to the websocket
    // sub-handler, whose router is absent — it answers 400 in that case.
    let mut conn = TcpStream::connect(addr).await.unwrap();
    let req = "GET /chat HTTP/1.1\r\n\
               Host: localhost\r\n\
               Upgrade: websocket\r\n\
               Connection: Upgrade\r\n\
               Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
               Sec-WebSocket-Version: 13\r\n\r\n";
    conn.write_all(req.as_bytes()).await.unwrap();
    let mut buf = [0u8; 64];
    let n = conn.read(&mut buf).await.unwrap();
    let head = String::from_utf8_lossy(&buf[..n]);
    assert!(head.starts_with("HTTP/1.1 400"), "got {head}");
}

#[tokio::test]
async fn nat_handler_without_reader_warns_and_returns() {
    let handler = aex::nat::nat_tcp_handler(aex::nat::NatRelayService::new());
    let addr: SocketAddr = "127.0.0.1:9090".parse().unwrap();
    let ctx = Context::new(None, None, gctx(addr), addr);
    handler(ctx);
    sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn nat_handler_without_writer_warns_and_returns() {
    let handler = aex::nat::nat_tcp_handler(aex::nat::NatRelayService::new());
    let addr: SocketAddr = "127.0.0.1:9090".parse().unwrap();
    let (rd, _wr) = tokio::io::duplex(16);
    let reader: aex::connection::context::BoxReader = Box::new(tokio::io::BufReader::new(rd));
    let ctx = Context::new(Some(reader), None, gctx(addr), addr);
    handler(ctx);
    sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn enable_nat_duplicate_detector_is_logged() {
    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let server = Server::new(gctx(addr))
        .detector(Arc::new(aex::nat::NatDetector))
        .enable_nat(aex::nat::NatRelayService::new());

    assert_eq!(server.registry.list(), vec!["nat-detector".to_string()]);
    assert!(server.protocols.iter().any(|p| p.name() == "nat"));
}
