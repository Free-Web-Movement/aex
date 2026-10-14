//! Extra integration tests for the SOCKS4/4a/5 proxy in `src/proxy/socks.rs`.
//!
//! Two layers are covered:
//!   * the public `SocksDetector` (`ProtocolDetector`) verdicts, and
//!   * the full `socks_tcp_handler` session driven over in-memory duplex
//!     streams with real loopback upstreams.

#![cfg(feature = "proxy")]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream};
use tokio::net::TcpListener;

use aex::connection::context::{BoxReader, BoxWriter, Context};
use aex::connection::global::GlobalContext;
use aex::detect::{DetectionState, ProtocolDetector, Verdict};
use aex::proxy::socks::SocksVersion;
use aex::proxy::{ProxyAuthorizer, SocksDetector, socks_tcp_handler};

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn free_addr() -> SocketAddr {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
}

fn socks_ctx() -> (Context, DuplexStream, DuplexStream) {
    let addr = free_addr();
    let global = Arc::new(GlobalContext::new(addr, None));
    let (client_tx, handler_rx) = tokio::io::duplex(1 << 16);
    let (handler_tx, client_rx) = tokio::io::duplex(1 << 16);
    let reader: BoxReader = Box::new(BufReader::new(handler_rx));
    let writer: BoxWriter = Box::new(handler_tx);
    let ctx = Context::new(Some(reader), Some(writer), global, addr);
    (ctx, client_tx, client_rx)
}

async fn read_exact_t(rx: &mut DuplexStream, buf: &mut [u8]) {
    tokio::time::timeout(Duration::from_secs(3), rx.read_exact(buf))
        .await
        .expect("timed out waiting for bytes")
        .expect("read failed");
}

async fn join_t(join: tokio::task::JoinHandle<()>) {
    tokio::time::timeout(Duration::from_secs(3), join)
        .await
        .expect("socks session should finish")
        .expect("session task panicked");
}

/// One-shot byte echo upstream. It echoes the first chunk it receives and then
/// closes, so the relay's upstream read reaches EOF and the session can finish.
async fn spawn_echo_upstream() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut tmp = [0u8; 1024];
                let n = sock.read(&mut tmp).await.unwrap_or(0);
                if n > 0 {
                    let _ = sock.write_all(&tmp[..n]).await;
                }
                let _ = sock.shutdown().await;
            });
        }
    });
    addr
}

fn v4_octets(addr: SocketAddr) -> [u8; 4] {
    match addr {
        SocketAddr::V4(v4) => v4.ip().octets(),
        _ => panic!("expected IPv4"),
    }
}

fn v5_greeting(methods: &[u8]) -> Vec<u8> {
    let mut g = vec![0x05, methods.len() as u8];
    g.extend_from_slice(methods);
    g
}

fn v5_connect_v4(ip: [u8; 4], port: u16) -> Vec<u8> {
    let mut r = vec![0x05, 0x01, 0x00, 0x01];
    r.extend_from_slice(&ip);
    r.extend_from_slice(&port.to_be_bytes());
    r
}

fn v5_connect_domain(host: &str, port: u16) -> Vec<u8> {
    let mut r = vec![0x05, 0x01, 0x00, 0x03, host.len() as u8];
    r.extend_from_slice(host.as_bytes());
    r.extend_from_slice(&port.to_be_bytes());
    r
}

fn v5_connect_v6(ip: [u8; 16], port: u16) -> Vec<u8> {
    let mut r = vec![0x05, 0x01, 0x00, 0x04];
    r.extend_from_slice(&ip);
    r.extend_from_slice(&port.to_be_bytes());
    r
}

fn v4_connect(ip: [u8; 4], port: u16, user: &str) -> Vec<u8> {
    let mut r = vec![0x04, 0x01];
    r.extend_from_slice(&port.to_be_bytes());
    r.extend_from_slice(&ip);
    r.extend_from_slice(user.as_bytes());
    r.push(0);
    r
}

fn v4a_connect(port: u16, user: &str, domain: &str) -> Vec<u8> {
    let mut r = vec![0x04, 0x01];
    r.extend_from_slice(&port.to_be_bytes());
    r.extend_from_slice(&[0, 0, 0, 1]);
    r.extend_from_slice(user.as_bytes());
    r.push(0);
    r.extend_from_slice(domain.as_bytes());
    r.push(0);
    r
}

fn alice() -> ProxyAuthorizer {
    Arc::new(|u: &str, p: &str| u == "alice" && p == "wonderland")
}

// ---------------------------------------------------------------------------
// SocksDetector verdicts
// ---------------------------------------------------------------------------

#[test]
fn socks_detector_metadata_is_stable() {
    let d = SocksDetector;
    assert_eq!(d.name(), "socks");
    assert_eq!(d.protocol(), "socks");
    assert_eq!(d.max_need(), Some(257));
}

#[test]
fn socks_detector_v5_paths() {
    let d = SocksDetector;

    let mut st = DetectionState::new();
    assert_eq!(d.detect(&[], &mut st), Verdict::Pass);

    let mut st = DetectionState::new();
    assert_eq!(d.detect(&[0x05], &mut st), Verdict::NeedMore(1));

    let mut st = DetectionState::new();
    assert_eq!(d.detect(&[0x05, 0x00], &mut st), Verdict::Pass);

    let mut st = DetectionState::new();
    assert_eq!(d.detect(&[0x05, 0x02, 0x00], &mut st), Verdict::NeedMore(1));

    let mut st = DetectionState::new();
    assert_eq!(d.detect(&[0x05, 0x02, 0x00, 0x02], &mut st), Verdict::Match);
    assert_eq!(st.get_scratch::<SocksVersion>(), Some(SocksVersion::V5));
}

#[test]
fn socks_detector_v4_paths() {
    let d = SocksDetector;

    let mut st = DetectionState::new();
    assert_eq!(d.detect(&[0x04], &mut st), Verdict::NeedMore(1));

    let mut st = DetectionState::new();
    assert_eq!(d.detect(&[0x04, 0x03], &mut st), Verdict::Pass);

    let mut st = DetectionState::new();
    assert_eq!(
        d.detect(&[0x04, 0x01, 0x00, 0x50, 127, 0, 0], &mut st),
        Verdict::NeedMore(1)
    );

    let mut st = DetectionState::new();
    assert_eq!(
        d.detect(&[0x04, 0x01, 0x00, 0x50, 127, 0, 0, 1], &mut st),
        Verdict::Match
    );
    assert_eq!(st.get_scratch::<SocksVersion>(), Some(SocksVersion::V4));
}

#[test]
fn socks_detector_unknown_first_byte_passes() {
    let d = SocksDetector;
    let mut st = DetectionState::new();
    assert_eq!(d.detect(&[0x01, 0x02, 0x03], &mut st), Verdict::Pass);
}

// ---------------------------------------------------------------------------
// handler wiring
// ---------------------------------------------------------------------------

#[tokio::test]
async fn socks_handler_without_reader_bails() {
    let addr = free_addr();
    let global = Arc::new(GlobalContext::new(addr, None));
    let ctx = Context::new(None, Some(Box::new(tokio::io::sink())), global, addr);
    let h = socks_tcp_handler(None);
    join_t(h(ctx)).await;
}

#[tokio::test]
async fn socks_handler_without_writer_bails() {
    let addr = free_addr();
    let global = Arc::new(GlobalContext::new(addr, None));
    let (a, _b) = tokio::io::duplex(64);
    let reader: BoxReader = Box::new(BufReader::new(a));
    let ctx = Context::new(Some(reader), None, global, addr);
    let h = socks_tcp_handler(None);
    join_t(h(ctx)).await;
}

#[tokio::test]
async fn socks_bad_version_greeting_bails() {
    let (ctx, mut tx, _rx) = socks_ctx();
    let h = socks_tcp_handler(None);
    let join = h(ctx);
    tx.write_all(&[0x06, 0x01, 0x00]).await.unwrap();
    drop(tx);
    join_t(join).await;
}

// ---------------------------------------------------------------------------
// SOCKS5
// ---------------------------------------------------------------------------

#[tokio::test]
async fn socks5_no_auth_connect_success() {
    let upstream = spawn_echo_upstream().await;
    let (ctx, mut tx, mut rx) = socks_ctx();
    let join = socks_tcp_handler(None)(ctx);

    tx.write_all(&v5_greeting(&[0x00])).await.unwrap();
    let mut reply = [0u8; 2];
    read_exact_t(&mut rx, &mut reply).await;
    assert_eq!(reply, [0x05, 0x00]);

    tx.write_all(&v5_connect_v4(v4_octets(upstream), upstream.port()))
        .await
        .unwrap();
    let mut rep = [0u8; 10];
    read_exact_t(&mut rx, &mut rep).await;
    assert_eq!(rep[1], 0x00);

    tx.write_all(b"ping").await.unwrap();
    let mut echo = [0u8; 4];
    read_exact_t(&mut rx, &mut echo).await;
    assert_eq!(&echo, b"ping");

    drop(tx);
    drop(rx);
    join_t(join).await;
}

#[tokio::test]
async fn socks5_domain_connect_success() {
    let upstream = spawn_echo_upstream().await;
    let (ctx, mut tx, mut rx) = socks_ctx();
    let join = socks_tcp_handler(None)(ctx);

    tx.write_all(&v5_greeting(&[0x00])).await.unwrap();
    let mut reply = [0u8; 2];
    read_exact_t(&mut rx, &mut reply).await;
    assert_eq!(reply, [0x05, 0x00]);

    tx.write_all(&v5_connect_domain("localhost", upstream.port()))
        .await
        .unwrap();
    let mut rep = [0u8; 10];
    read_exact_t(&mut rx, &mut rep).await;
    assert_eq!(rep[1], 0x00);

    tx.write_all(b"dom").await.unwrap();
    let mut echo = [0u8; 3];
    read_exact_t(&mut rx, &mut echo).await;
    assert_eq!(&echo, b"dom");

    drop(tx);
    drop(rx);
    join_t(join).await;
}

#[tokio::test]
async fn socks5_ipv6_connect_success() {
    let Ok(listener) = TcpListener::bind("[::1]:0").await else {
        eprintln!("IPv6 loopback unavailable, skipping");
        return;
    };
    let upstream = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut tmp = [0u8; 1024];
                let n = sock.read(&mut tmp).await.unwrap_or(0);
                if n > 0 {
                    let _ = sock.write_all(&tmp[..n]).await;
                }
                let _ = sock.shutdown().await;
            });
        }
    });

    let (ctx, mut tx, mut rx) = socks_ctx();
    let join = socks_tcp_handler(None)(ctx);

    tx.write_all(&v5_greeting(&[0x00])).await.unwrap();
    let mut reply = [0u8; 2];
    read_exact_t(&mut rx, &mut reply).await;
    assert_eq!(reply, [0x05, 0x00]);

    let mut ip = [0u8; 16];
    ip[15] = 1; // ::1
    tx.write_all(&v5_connect_v6(ip, upstream.port()))
        .await
        .unwrap();
    let mut rep = [0u8; 10];
    read_exact_t(&mut rx, &mut rep).await;
    assert_eq!(rep[1], 0x00, "IPv6 CONNECT should succeed");

    tx.write_all(b"v6").await.unwrap();
    let mut echo = [0u8; 2];
    read_exact_t(&mut rx, &mut echo).await;
    assert_eq!(&echo, b"v6");

    drop(tx);
    drop(rx);
    join_t(join).await;
}

#[tokio::test]
async fn socks5_user_pass_auth_success() {
    let upstream = spawn_echo_upstream().await;
    let (ctx, mut tx, mut rx) = socks_ctx();
    let join = socks_tcp_handler(Some(alice()))(ctx);

    tx.write_all(&v5_greeting(&[0x00, 0x02])).await.unwrap();
    let mut reply = [0u8; 2];
    read_exact_t(&mut rx, &mut reply).await;
    assert_eq!(reply, [0x05, 0x02]);

    let mut sub = vec![0x01, 5];
    sub.extend_from_slice(b"alice");
    sub.push(10);
    sub.extend_from_slice(b"wonderland");
    tx.write_all(&sub).await.unwrap();
    let mut verdict = [0u8; 2];
    read_exact_t(&mut rx, &mut verdict).await;
    assert_eq!(verdict, [0x01, 0x00]);

    tx.write_all(&v5_connect_v4(v4_octets(upstream), upstream.port()))
        .await
        .unwrap();
    let mut rep = [0u8; 10];
    read_exact_t(&mut rx, &mut rep).await;
    assert_eq!(rep[1], 0x00);

    tx.write_all(b"auth").await.unwrap();
    let mut echo = [0u8; 4];
    read_exact_t(&mut rx, &mut echo).await;
    assert_eq!(&echo, b"auth");

    drop(tx);
    drop(rx);
    join_t(join).await;
}

#[tokio::test]
async fn socks5_user_pass_auth_failure() {
    let (ctx, mut tx, mut rx) = socks_ctx();
    let join = socks_tcp_handler(Some(alice()))(ctx);

    tx.write_all(&v5_greeting(&[0x02])).await.unwrap();
    let mut reply = [0u8; 2];
    read_exact_t(&mut rx, &mut reply).await;
    assert_eq!(reply, [0x05, 0x02]);

    let mut sub = vec![0x01, 5];
    sub.extend_from_slice(b"alice");
    sub.push(3);
    sub.extend_from_slice(b"bad");
    tx.write_all(&sub).await.unwrap();
    let mut verdict = [0u8; 2];
    read_exact_t(&mut rx, &mut verdict).await;
    assert_ne!(verdict[1], 0x00, "wrong password must be rejected");

    drop(tx);
    drop(rx);
    join_t(join).await;
}

#[tokio::test]
async fn socks5_no_acceptable_method_rejected() {
    let (ctx, mut tx, mut rx) = socks_ctx();
    let join = socks_tcp_handler(Some(alice()))(ctx);

    // Client only offers no-auth; server requires user/pass.
    tx.write_all(&v5_greeting(&[0x00])).await.unwrap();
    let mut reply = [0u8; 2];
    read_exact_t(&mut rx, &mut reply).await;
    assert_eq!(reply, [0x05, 0xFF]);

    drop(tx);
    drop(rx);
    join_t(join).await;
}

#[tokio::test]
async fn socks5_open_proxy_rejects_userpass_only() {
    let (ctx, mut tx, mut rx) = socks_ctx();
    let join = socks_tcp_handler(None)(ctx);

    // Open proxy, but the client only offers user/pass.
    tx.write_all(&v5_greeting(&[0x02])).await.unwrap();
    let mut reply = [0u8; 2];
    read_exact_t(&mut rx, &mut reply).await;
    assert_eq!(reply, [0x05, 0xFF]);

    drop(tx);
    drop(rx);
    join_t(join).await;
}

#[tokio::test]
async fn socks5_unsupported_command_rejected() {
    let (ctx, mut tx, mut rx) = socks_ctx();
    let join = socks_tcp_handler(None)(ctx);

    tx.write_all(&v5_greeting(&[0x00])).await.unwrap();
    let mut reply = [0u8; 2];
    read_exact_t(&mut rx, &mut reply).await;
    assert_eq!(reply, [0x05, 0x00]);

    // CMD = 0x02 (BIND), unsupported.
    tx.write_all(&[0x05, 0x02, 0x00, 0x01, 127, 0, 0, 1, 0x00, 0x50])
        .await
        .unwrap();
    let mut rep = [0u8; 10];
    read_exact_t(&mut rx, &mut rep).await;
    assert_eq!(rep[1], 0x07);

    drop(tx);
    drop(rx);
    join_t(join).await;
}

#[tokio::test]
async fn socks5_unsupported_atyp_rejected() {
    let (ctx, mut tx, mut rx) = socks_ctx();
    let join = socks_tcp_handler(None)(ctx);

    tx.write_all(&v5_greeting(&[0x00])).await.unwrap();
    let mut reply = [0u8; 2];
    read_exact_t(&mut rx, &mut reply).await;
    assert_eq!(reply, [0x05, 0x00]);

    // ATYP = 0x09, unsupported.
    tx.write_all(&[0x05, 0x01, 0x00, 0x09, 1, 2, 3, 4, 0x00, 0x50])
        .await
        .unwrap();
    let mut rep = [0u8; 10];
    read_exact_t(&mut rx, &mut rep).await;
    assert_eq!(rep[1], 0x08);

    drop(tx);
    drop(rx);
    join_t(join).await;
}

#[tokio::test]
async fn socks5_refused_target_reports_failure() {
    let dead = free_addr();
    let (ctx, mut tx, mut rx) = socks_ctx();
    let join = socks_tcp_handler(None)(ctx);

    tx.write_all(&v5_greeting(&[0x00])).await.unwrap();
    let mut reply = [0u8; 2];
    read_exact_t(&mut rx, &mut reply).await;
    assert_eq!(reply, [0x05, 0x00]);

    tx.write_all(&v5_connect_v4([127, 0, 0, 1], dead.port()))
        .await
        .unwrap();
    let mut rep = [0u8; 10];
    read_exact_t(&mut rx, &mut rep).await;
    assert_ne!(rep[1], 0x00, "refused target must not report success");

    drop(tx);
    drop(rx);
    join_t(join).await;
}

#[tokio::test]
async fn socks5_domain_resolution_failure_reports_failure() {
    let (ctx, mut tx, mut rx) = socks_ctx();
    let join = socks_tcp_handler(None)(ctx);

    tx.write_all(&v5_greeting(&[0x00])).await.unwrap();
    let mut reply = [0u8; 2];
    read_exact_t(&mut rx, &mut reply).await;
    assert_eq!(reply, [0x05, 0x00]);

    tx.write_all(&v5_connect_domain("nonexistent.invalid", 80))
        .await
        .unwrap();
    let mut rep = [0u8; 10];
    read_exact_t(&mut rx, &mut rep).await;
    assert_ne!(rep[1], 0x00, "unresolvable domain must not report success");

    drop(tx);
    drop(rx);
    join_t(join).await;
}

// ---------------------------------------------------------------------------
// SOCKS4 / 4a
// ---------------------------------------------------------------------------

#[tokio::test]
async fn socks4_connect_success() {
    let upstream = spawn_echo_upstream().await;
    let (ctx, mut tx, mut rx) = socks_ctx();
    let join = socks_tcp_handler(None)(ctx);

    tx.write_all(&v4_connect(v4_octets(upstream), upstream.port(), "user"))
        .await
        .unwrap();
    let mut rep = [0u8; 8];
    read_exact_t(&mut rx, &mut rep).await;
    assert_eq!(rep[1], 0x5A);

    tx.write_all(b"v4").await.unwrap();
    let mut echo = [0u8; 2];
    read_exact_t(&mut rx, &mut echo).await;
    assert_eq!(&echo, b"v4");

    drop(tx);
    drop(rx);
    join_t(join).await;
}

#[tokio::test]
async fn socks4a_domain_connect_success() {
    let upstream = spawn_echo_upstream().await;
    let (ctx, mut tx, mut rx) = socks_ctx();
    let join = socks_tcp_handler(None)(ctx);

    tx.write_all(&v4a_connect(upstream.port(), "user", "localhost"))
        .await
        .unwrap();
    let mut rep = [0u8; 8];
    read_exact_t(&mut rx, &mut rep).await;
    assert_eq!(rep[1], 0x5A);

    tx.write_all(b"v4a").await.unwrap();
    let mut echo = [0u8; 3];
    read_exact_t(&mut rx, &mut echo).await;
    assert_eq!(&echo, b"v4a");

    drop(tx);
    drop(rx);
    join_t(join).await;
}

#[tokio::test]
async fn socks4_non_connect_rejected() {
    let (ctx, mut tx, mut rx) = socks_ctx();
    let join = socks_tcp_handler(None)(ctx);

    // VER=4, CMD=0x02 (BIND), unsupported.
    let mut req = vec![0x04, 0x02, 0x00, 0x50, 127, 0, 0, 1];
    req.extend_from_slice(b"user\0");
    tx.write_all(&req).await.unwrap();
    let mut rep = [0u8; 8];
    read_exact_t(&mut rx, &mut rep).await;
    assert_eq!(rep[1], 0x5B);

    drop(tx);
    drop(rx);
    join_t(join).await;
}

#[tokio::test]
async fn socks4_refused_target_reports_failure() {
    let dead = free_addr();
    let (ctx, mut tx, mut rx) = socks_ctx();
    let join = socks_tcp_handler(None)(ctx);

    tx.write_all(&v4_connect([127, 0, 0, 1], dead.port(), "user"))
        .await
        .unwrap();
    let mut rep = [0u8; 8];
    read_exact_t(&mut rx, &mut rep).await;
    assert_eq!(rep[1], 0x5B);

    drop(tx);
    drop(rx);
    join_t(join).await;
}

#[tokio::test]
async fn socks4a_domain_refused_reports_failure() {
    let dead = free_addr();
    let (ctx, mut tx, mut rx) = socks_ctx();
    let join = socks_tcp_handler(None)(ctx);

    tx.write_all(&v4a_connect(dead.port(), "user", "localhost"))
        .await
        .unwrap();
    let mut rep = [0u8; 8];
    read_exact_t(&mut rx, &mut rep).await;
    assert_eq!(rep[1], 0x5B);

    drop(tx);
    drop(rx);
    join_t(join).await;
}

#[tokio::test]
async fn socks4_identifier_too_long_bails() {
    let (ctx, mut tx, _rx) = socks_ctx();
    let join = socks_tcp_handler(None)(ctx);

    // 8-byte header followed by a >255-byte identifier with no NUL terminator.
    let mut req = vec![0x04, 0x01, 0x00, 0x50, 127, 0, 0, 1];
    req.extend(std::iter::repeat_n(b'x', 300));
    tx.write_all(&req).await.unwrap();
    drop(tx);

    join_t(join).await;
}
