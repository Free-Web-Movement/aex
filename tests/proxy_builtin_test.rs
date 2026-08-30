//! Integration tests for the built-in proxy services (feature = "proxy").
//!
//! One UnifiedServer instance simultaneously serves:
//!   * a website            — origin-form requests
//!   * an HTTP forward proxy — absolute-form requests
//!   * an HTTP tunnel       — CONNECT host:port
//!   * SOCKS4/4a/5 CONNECT  — greeting-claimed connections
//! The client's first bytes decide which service handles the connection.

#![cfg(feature = "proxy")]

use std::net::SocketAddr;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use aex::connection::global::GlobalContext;
use aex::unified::UnifiedServer;

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn free_addr() -> SocketAddr {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
}

/// Tiny origin server answering every request with a fixed body.
async fn spawn_upstream(body: &'static str) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                // Echo which path was requested so tests can verify routing.
                let path = req.split_whitespace().nth(1).unwrap_or("-");
                let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body} path={path}", body.len() + path.len() + 6);
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    addr
}

fn spawn_server(addr: SocketAddr, configure: impl FnOnce(UnifiedServer) -> UnifiedServer) {
    let globals = Arc::new(GlobalContext::new(addr, None));
    let server = configure(UnifiedServer::new(addr, globals));
    tokio::spawn(async move {
        if let Err(e) = server.start().await {
            eprintln!("server exited: {e}");
        }
    });
}

async fn wait_listening(addr: SocketAddr) {
    for _ in 0..100 {
        if TcpStream::connect(addr).await.is_ok() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("server never became reachable at {addr}");
}

/// Read until EOF or end-of-headers+content-length; returns whole text.
async fn read_all(sock: &mut TcpStream) -> String {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 2048];
    loop {
        match sock.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if String::from_utf8_lossy(&buf).contains("\r\n\r\n") && !buf.ends_with(b"\r\n\r\n") {
                    // got headers plus some body — good enough for asserts
                    break;
                }
            }
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn one_port_serves_website_and_http_proxy() {
    let upstream = spawn_upstream("UPSTREAM").await;
    let addr = free_addr();
    spawn_server(addr, |s| s.enable_http_proxy());
    wait_listening(addr).await;

    // 1) Origin-form → website traffic: no handler registered, so we expect
    //    aex's failure response rather than a proxy forward.
    let mut site = TcpStream::connect(addr).await.unwrap();
    site.write_all(b"GET /index.html HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();
    let resp = read_all(&mut site).await;
    assert!(resp.starts_with("HTTP/1.1 "), "got: {resp}");
    assert!(!resp.contains("UPSTREAM"), "origin-form must not be proxied");

    // 2) Absolute-form → forwarded to the upstream.
    let mut proxy_req = TcpStream::connect(addr).await.unwrap();
    let url = format!(
        "GET http://{}{} HTTP/1.1\r\nHost: ignored\r\nX-Keep: me\r\n\r\n",
        upstream, "/hello"
    );
    proxy_req.write_all(url.as_bytes()).await.unwrap();
    let resp = read_all(&mut proxy_req).await;
    assert!(resp.contains("200 OK"), "got: {resp}");
    assert!(resp.contains("path=/hello"), "got: {resp}");
}

#[tokio::test]
async fn connect_tunnel_relays_raw_bytes() {
    let upstream = spawn_upstream("TUNNELED").await;
    let addr = free_addr();
    spawn_server(addr, |s| s.enable_http_proxy());
    wait_listening(addr).await;

    let mut sock = TcpStream::connect(addr).await.unwrap();
    sock.write_all(format!("CONNECT {} HTTP/1.1\r\nHost: {}\r\n\r\n", upstream, upstream).as_bytes())
        .await
        .unwrap();

    // Read just the head of the 2xx response.
    let mut head = vec![0u8; 64];
    let n = sock.read(&mut head).await.unwrap();
    assert!(String::from_utf8_lossy(&head[..n]).contains("200"), "got: {:?}", &head[..n]);

    // Inside the tunnel speak plain HTTP to the upstream.
    sock.write_all(format!("GET /via-tunnel HTTP/1.1\r\nHost: {}\r\n\r\n", upstream).as_bytes())
        .await
        .unwrap();
    let resp = read_all(&mut sock).await;
    assert!(resp.contains("TUNNELED") && resp.contains("path=/via-tunnel"), "got: {resp}");
}

#[tokio::test]
async fn socks5_no_auth_connect() {
    let upstream = spawn_upstream("SOCKS5").await;
    let addr = free_addr();
    spawn_server(addr, |s| s.enable_socks_proxy());
    wait_listening(addr).await;

    let mut s = TcpStream::connect(addr).await.unwrap();
    // Greeting: no-auth only.
    s.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
    let mut reply = [0u8; 2];
    s.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply, [0x05, 0x00]);

    // CONNECT to upstream via IPv4 ATYP.
    let ip = match upstream {
        SocketAddr::V4(v4) => v4.ip().octets(),
        _ => panic!("expected v4"),
    };
    let mut req = vec![0x05, 0x01, 0x00, 0x01];
    req.extend_from_slice(&ip);
    req.extend_from_slice(&upstream.port().to_be_bytes());
    s.write_all(&req).await.unwrap();
    let mut rep = [0u8; 10];
    s.read_exact(&mut rep).await.unwrap();
    assert_eq!(rep[1], 0x00, "connect must succeed");

    // Tunneled HTTP through SOCKS.
    s.write_all(format!("GET /socks5 HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let resp = read_all(&mut s).await;
    assert!(resp.contains("SOCKS5") && resp.contains("path=/socks5"), "got: {resp}");
}

#[tokio::test]
async fn socks5_with_user_pass_auth() {
    let upstream = spawn_upstream("AUTH").await;
    let addr = free_addr();
    spawn_server(addr, |s| {
        s.enable_socks_proxy()
            .proxy_authenticator(Arc::new(|u, p| u == "alice" && p == "wonderland"))
    });
    wait_listening(addr).await;

    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(&[0x05, 0x02, 0x00, 0x02]).await.unwrap(); // offer none+userpass
    let mut reply = [0u8; 2];
    s.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply, [0x05, 0x02], "server must pick user/pass");

    // RFC 1929 subnegotiation.
    let mut sub = vec![0x01, 5];
    sub.extend_from_slice(b"alice");
    sub.push(10);
    sub.extend_from_slice(b"wonderland");
    s.write_all(&sub).await.unwrap();
    let mut verdict = [0u8; 2];
    s.read_exact(&mut verdict).await.unwrap();
    assert_eq!(verdict, [0x01, 0x00], "auth must succeed");

    // Wrong password gets rejected.
    let mut bad = TcpStream::connect(addr).await.unwrap();
    bad.write_all(&[0x05, 0x01, 0x02]).await.unwrap();
    let mut r2 = [0u8; 2];
    bad.read_exact(&mut r2).await.unwrap();
    assert_eq!(r2, [0x05, 0x02]);
    let mut wrong = vec![0x01, 5];
    wrong.extend_from_slice(b"alice");
    wrong.push(3);
    wrong.extend_from_slice(b"xyz");
    bad.write_all(&wrong).await.unwrap();
    let mut v2 = [0u8; 2];
    bad.read_exact(&mut v2).await.unwrap();
    assert_ne!(v2[1], 0, "wrong credentials must fail (non-zero status)");
}

#[tokio::test]
async fn socks4_and_v4a_domain() {
    // Upstream on a domain we control via /etc/hosts? Not available — use
    // 127.0.0.1 for plain v4 and exercise v4a with "localhost".
    let upstream = spawn_upstream("SOCKS4").await;
    let port = upstream.port();
    let ip = match upstream {
        SocketAddr::V4(v4) => v4.ip().octets(),
        _ => panic!("expected v4"),
    };

    let addr = free_addr();
    spawn_server(addr, |s| s.enable_socks_proxy());
    wait_listening(addr).await;

    // --- plain SOCKS4 ---
    let mut s = TcpStream::connect(addr).await.unwrap();
    let mut req = vec![0x04, 0x01];
    req.extend_from_slice(&port.to_be_bytes());
    req.extend_from_slice(&ip);
    req.extend_from_slice(b"user\0");
    s.write_all(&req).await.unwrap();
    let mut rep = [0u8; 8];
    s.read_exact(&mut rep).await.unwrap();
    assert_eq!(rep[1], 0x5A, "v4 connect granted");

    s.write_all(b"GET /v4 HTTP/1.1\r\nHost: x\r\n\r\n").await.unwrap();
    let resp = read_all(&mut s).await;
    assert!(resp.contains("SOCKS4") && resp.contains("path=/v4"), "got: {resp}");

    // --- SOCKS4a with hostname ---
    let mut s = TcpStream::connect(addr).await.unwrap();
    let mut req = vec![0x04, 0x01];
    req.extend_from_slice(&port.to_be_bytes());
    req.extend_from_slice(&[0, 0, 0, 1]); // 0.0.0.x → domain follows
    req.extend_from_slice(b"user\0localhost\0");
    s.write_all(&req).await.unwrap();
    let mut rep = [0u8; 8];
    s.read_exact(&mut rep).await.unwrap();
    assert_eq!(rep[1], 0x5A, "v4a connect granted");

    s.write_all(b"GET /v4a HTTP/1.1\r\nHost: x\r\n\r\n").await.unwrap();
    let resp = read_all(&mut s).await;
    assert!(resp.contains("SOCKS4") && resp.contains("path=/v4a"), "got: {resp}");
}

// ---------------------------------------------------------------------------
// SOCKS error branches
// ---------------------------------------------------------------------------

#[tokio::test]
async fn socks5_no_acceptable_auth_rejected() {
    let addr = free_addr();
    spawn_server(addr, |s| {
        s.enable_socks_proxy()
            .proxy_authenticator(Arc::new(|u, p| u == "alice" && p == "wonderland"))
    });
    wait_listening(addr).await;

    // Client only offers no-auth (0x00); server requires user/pass.
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
    let mut reply = [0u8; 2];
    s.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply, [0x05, 0xFF], "server must reject when no acceptable method");
}

#[tokio::test]
async fn socks5_unsupported_command_rejected() {
    let addr = free_addr();
    spawn_server(addr, |s| s.enable_socks_proxy());
    wait_listening(addr).await;

    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
    let mut reply = [0u8; 2];
    s.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply, [0x05, 0x00]);

    // CMD = 0x02 (BIND), which is not supported.
    let mut req = vec![0x05, 0x02, 0x00, 0x01, 127, 0, 0, 1, 0x00, 0x50];
    s.write_all(&req).await.unwrap();
    let mut rep = [0u8; 10];
    s.read_exact(&mut rep).await.unwrap();
    assert_eq!(rep[1], 0x07, "command not supported REP=0x07");
}

#[tokio::test]
async fn socks5_unsupported_atyp_rejected() {
    let addr = free_addr();
    spawn_server(addr, |s| s.enable_socks_proxy());
    wait_listening(addr).await;

    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
    let mut reply = [0u8; 2];
    s.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply, [0x05, 0x00]);

    // ATYP = 0x09 (unsupported).
    let mut req = vec![0x05, 0x01, 0x00, 0x09, 1, 2, 3, 4, 0x00, 0x50];
    s.write_all(&req).await.unwrap();
    let mut rep = [0u8; 10];
    s.read_exact(&mut rep).await.unwrap();
    assert_eq!(rep[1], 0x08, "ATYP not supported REP=0x08");
}

#[tokio::test]
async fn socks4_non_connect_rejected() {
    let addr = free_addr();
    spawn_server(addr, |s| s.enable_socks_proxy());
    wait_listening(addr).await;

    // VER=4, CMD=0x02 (BIND), unsupported.
    let mut s = TcpStream::connect(addr).await.unwrap();
    let mut req = vec![0x04, 0x02, 0x00, 0x50, 127, 0, 0, 1];
    req.extend_from_slice(b"user\0");
    s.write_all(&req).await.unwrap();
    let mut rep = [0u8; 8];
    s.read_exact(&mut rep).await.unwrap();
    assert_eq!(rep[1], 0x5B, "v4 non-CONNECT rejected with 0x5B");
}

#[tokio::test]
async fn socks5_connect_to_unreachable_reports_failure() {
    let addr = free_addr();
    spawn_server(addr, |s| s.enable_socks_proxy());
    wait_listening(addr).await;

    // Reserve a port then drop the listener so connect gets RST (deterministic).
    let dead = free_addr();

    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
    let mut reply = [0u8; 2];
    s.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply, [0x05, 0x00]);

    // ATYP=1, 127.0.0.1:<dead-port> → connection refused → failure frame.
    let mut req = vec![0x05, 0x01, 0x00, 0x01, 127, 0, 0, 1];
    req.extend_from_slice(&dead.port().to_be_bytes());
    s.write_all(&req).await.unwrap();
    let mut rep = [0u8; 10];
    s.read_exact(&mut rep).await.unwrap();
    assert_ne!(rep[1], 0x00, "unreachable target must not report success");
}

#[tokio::test]
async fn socks5_ipv6_atyp_connect() {
    let addr = free_addr();
    spawn_server(addr, |s| s.enable_socks_proxy());
    wait_listening(addr).await;

    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
    let mut reply = [0u8; 2];
    s.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply, [0x05, 0x00]);

    // ATYP=4, ::1:upstream (IPv6 loopback).
    let mut req = vec![0x05, 0x01, 0x00, 0x04];
    req.extend_from_slice(&[0u8; 15]);
    req.push(1u8); // ::1
    req.extend_from_slice(&[0x00, 0x50]);
    s.write_all(&req).await.unwrap();
    let mut rep = [0u8; 10];
    s.read_exact(&mut rep).await.unwrap();
    assert_eq!(rep[1], 0x00, "IPv6 CONNECT must report success, got REP={}", rep[1]);
}

#[tokio::test]
async fn socks5_domain_atyp_connect() {
    let upstream = spawn_upstream("SOCKS5-DOMAIN").await;
    let addr = free_addr();
    spawn_server(addr, |s| s.enable_socks_proxy());
    wait_listening(addr).await;

    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
    let mut reply = [0u8; 2];
    s.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply, [0x05, 0x00]);

    // ATYP=3 (domain), "localhost":upstream-port.
    let mut req = vec![0x05, 0x01, 0x00, 0x03];
    let host = b"localhost";
    req.push(host.len() as u8);
    req.extend_from_slice(host);
    req.extend_from_slice(&upstream.port().to_be_bytes());
    s.write_all(&req).await.unwrap();
    let mut rep = [0u8; 10];
    s.read_exact(&mut rep).await.unwrap();
    assert_eq!(rep[1], 0x00, "domain CONNECT must succeed, got REP={}", rep[1]);

    // Tunnel through it.
    s.write_all(b"GET /domain HTTP/1.1\r\nHost: x\r\n\r\n").await.unwrap();
    let resp = read_all(&mut s).await;
    assert!(resp.contains("SOCKS5-DOMAIN") && resp.contains("path=/domain"), "got: {resp}");
}
