//! End-to-end tests for the pluggable protocol detection pipeline over real
//! TCP connections: manual detector registration, custom protocol handlers,
//! Forward-mode (NAT-style) termination, link-state propagation, and runtime
//! registry mutation while the server is running.

use aex::connection::context::Context;
use aex::connection::global::GlobalContext;
use aex::unified::{
    DetectionState, DetectorMode, DetectorRegistry, ProtocolDetector, UnifiedServer, Verdict,
};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{Duration, sleep};

/// Grab a free loopback port by binding and dropping a listener.
async fn free_addr() -> SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    drop(l);
    addr
}

/// Matches a fixed magic prefix; counts how many times it was consulted.
struct MagicDetector {
    magic: &'static [u8],
    protocol: &'static str,
    mode: DetectorMode,
    consultations: Arc<AtomicUsize>,
}

impl MagicDetector {
    fn standard(magic: &'static [u8], protocol: &'static str) -> (Self, Arc<AtomicUsize>) {
        let c = Arc::new(AtomicUsize::new(0));
        (
            Self {
                magic,
                protocol,
                mode: DetectorMode::Standard,
                consultations: c.clone(),
            },
            c,
        )
    }
}

impl ProtocolDetector for MagicDetector {
    fn name(&self) -> &str {
        self.protocol
    }

    fn protocol(&self) -> &str {
        self.protocol
    }

    fn mode(&self) -> DetectorMode {
        self.mode
    }

    fn detect(&self, buf: &[u8], _state: &mut DetectionState) -> Verdict {
        self.consultations.fetch_add(1, Ordering::SeqCst);
        if buf.starts_with(self.magic) {
            Verdict::Match
        } else if self.magic.starts_with(buf) && !buf.is_empty() {
            Verdict::NeedMore(self.magic.len() - buf.len())
        } else {
            Verdict::Pass
        }
    }
}

#[tokio::test]
async fn custom_detector_routes_to_custom_handler() {
    let addr = free_addr().await;
    let hits = Arc::new(AtomicUsize::new(0));

    let handler_hits = hits.clone();
    let (detector, consults) = MagicDetector::standard(b"MAGIC1", "magic-proto");

    let server = UnifiedServer::new(addr, Arc::new(GlobalContext::new(addr, None)))
        .detector(Arc::new(detector))
        .custom_handler(
            "magic-proto",
            Arc::new(move |mut ctx| {
                let hits = handler_hits.clone();
                tokio::spawn(async move {
                    // Link state must be attached to the context.
                    let claim_protocol = ctx
                        .local
                        .get_ref::<DetectionState>()
                        .and_then(|s| s.claim().map(|c| c.protocol.to_string()));
                    assert_eq!(claim_protocol.as_deref(), Some("magic-proto"));

                    let mut buf = [0u8; 64];
                    let n = ctx.reader.as_mut().unwrap().read(&mut buf).await.unwrap_or(0);
                    assert_eq!(&buf[..n], b"MAGIC1-payload");
                    ctx.writer
                        .as_mut()
                        .unwrap()
                        .write_all(b"magic-ack")
                        .await
                        .ok();
                    hits.fetch_add(1, Ordering::SeqCst);
                })
            }),
        );

    tokio::spawn(async move { server.start().await.ok() });
    sleep(Duration::from_millis(100)).await;

    let mut conn = TcpStream::connect(addr).await.unwrap();
    conn.write_all(b"MAGIC1-payload").await.unwrap();
    let mut buf = [0u8; 64];
    let n = conn.read(&mut buf).await.unwrap();
    assert_eq!(&buf[..n], b"magic-ack");
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    assert!(consults.load(Ordering::SeqCst) >= 1);
}

#[tokio::test]
async fn forward_mode_terminates_detection_and_forwards_directly() {
    let addr = free_addr().await;
    let nat_matches = Arc::new(AtomicUsize::new(0));
    let forward_hits = Arc::new(AtomicUsize::new(0));
    let http_hits = Arc::new(AtomicUsize::new(0));

    // A NAT-style stateful forwarder placed BEFORE the HTTP detectors.
    struct NatForwarder(Arc<AtomicUsize>);

    impl ProtocolDetector for NatForwarder {
        fn name(&self) -> &str {
            "nat"
        }
        fn protocol(&self) -> &str {
            "nat-flow"
        }
        fn mode(&self) -> DetectorMode {
            DetectorMode::Forward
        }
        fn detect(&self, buf: &[u8], _state: &mut DetectionState) -> Verdict {
            // Pretend tracked NAT flows announce themselves with this tag.
            if buf.starts_with(b"NAT-FLOW:") {
                self.0.fetch_add(1, Ordering::SeqCst);
                Verdict::Match
            } else {
                Verdict::Pass
            }
        }
    }

    let nat_hits = nat_matches.clone();
    let http_handler_hits = http_hits.clone();
    // The forwarder's handler echoes whatever it receives.
    let fw = forward_hits.clone();
    let server = UnifiedServer::new(addr, Arc::new(GlobalContext::new(addr, None)))
        .detector(Arc::new(NatForwarder(nat_hits)))
        .detector(Arc::new(aex::unified::Http11Detector))
        .http_handler(Arc::new(move |_ctx| {
            let http_hits = http_handler_hits.clone();
            Box::pin(async move {
                http_hits.fetch_add(1, Ordering::SeqCst);
                true
            })
        }))
        .custom_handler(
            "nat-flow",
            Arc::new(move |mut ctx| {
                let fw = fw.clone();
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let _ = ctx.reader.as_mut().unwrap().read_to_end(&mut buf).await;
                    ctx.writer.as_mut().unwrap().write_all(b"forwarded").await.ok();
                    fw.fetch_add(1, Ordering::SeqCst);
                })
            }),
        );

    tokio::spawn(async move { server.start().await.ok() });
    sleep(Duration::from_millis(100)).await;

    // Forwarded connection: claimed by the NAT detector, HTTP never runs.
    {
        let mut conn = TcpStream::connect(addr).await.unwrap();
        conn.write_all(b"NAT-FLOW:opaque-bytes").await.unwrap();
        conn.shutdown().await.ok();
        let mut buf = [0u8; 32];
        let n = conn.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"forwarded");
    }

    // Normal traffic still flows through the standard chain.
    {
        let mut conn = TcpStream::connect(addr).await.unwrap();
        conn.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n")
            .await
            .unwrap();
        conn.shutdown().await.ok();
        sleep(Duration::from_millis(200)).await;
    }

    assert_eq!(nat_matches.load(Ordering::SeqCst), 1);
    assert_eq!(forward_hits.load(Ordering::SeqCst), 1);
    assert_eq!(http_hits.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn empty_registry_is_pure_tcp_passthrough() {
    let addr = free_addr().await;
    let hits = Arc::new(AtomicUsize::new(0));
    let h = hits.clone();

    // No detectors registered at all: nothing is peeked, everything goes to
    // the TCP handler verbatim.
    let server = UnifiedServer::new(addr, Arc::new(GlobalContext::new(addr, None))).tcp_handler(
        Arc::new(move |mut ctx| {
            let h = h.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 64];
                let n = ctx.reader.as_mut().unwrap().read(&mut buf).await.unwrap_or(0);
                assert_eq!(&buf[..n], b"raw-bytes");
                h.fetch_add(1, Ordering::SeqCst);
            })
        }),
    );

    tokio::spawn(async move { server.start().await.ok() });
    sleep(Duration::from_millis(100)).await;

    let mut conn = TcpStream::connect(addr).await.unwrap();
    conn.write_all(b"raw-bytes").await.unwrap();
    sleep(Duration::from_millis(200)).await;
    assert_eq!(hits.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn runtime_unregister_stops_claiming() {
    let addr = free_addr().await;
    let registry = Arc::new(DetectorRegistry::new());
    let custom_hits = Arc::new(AtomicUsize::new(0));
    let tcp_hits = Arc::new(AtomicUsize::new(0));

    let (detector, _consults) = MagicDetector::standard(b"MAGIC2", "magic2");

    let ch = custom_hits.clone();
    let th = tcp_hits.clone();
    let server = UnifiedServer::new(addr, Arc::new(GlobalContext::new(addr, None)))
        .with_registry(registry.clone())
        .detector(Arc::new(detector))
        .custom_handler(
            "magic2",
            Arc::new(move |_ctx| {
                let ch = ch.clone();
                tokio::spawn(async move { ch.fetch_add(1, Ordering::SeqCst); })
            }),
        )
        // TCP fallback reads the (chained) initial bytes to distinguish a
        // genuine passthrough from a claimed-but-unhandled protocol.
        .tcp_handler(Arc::new(move |mut ctx| {
            let th = th.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 16];
                let n = ctx.reader.as_mut().unwrap().read(&mut buf).await.unwrap_or(0);
                if buf[..n].starts_with(b"MAGIC2-y") {
                    th.fetch_add(1, Ordering::SeqCst);
                }
            })
        }));

    tokio::spawn(async move { server.start().await.ok() });
    sleep(Duration::from_millis(100)).await;

    // While registered: claimed by magic2 → custom handler.
    {
        let mut conn = TcpStream::connect(addr).await.unwrap();
        conn.write_all(b"MAGIC2-x").await.unwrap();
        conn.shutdown().await.ok();
        sleep(Duration::from_millis(150)).await;
    }
    assert_eq!(custom_hits.load(Ordering::SeqCst), 1);
    assert_eq!(tcp_hits.load(Ordering::SeqCst), 0);

    // Unregister at runtime: next connection falls through to TCP.
    assert!(registry.unregister("magic2"));
    {
        let mut conn = TcpStream::connect(addr).await.unwrap();
        conn.write_all(b"MAGIC2-y").await.unwrap();
        conn.shutdown().await.ok();
        sleep(Duration::from_millis(150)).await;
    }
    assert_eq!(custom_hits.load(Ordering::SeqCst), 1);
    assert_eq!(tcp_hits.load(Ordering::SeqCst), 1);
}
