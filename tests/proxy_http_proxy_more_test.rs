//! Extra integration tests for the HTTP forward proxy in `src/proxy/http_proxy.rs`.
//!
//! These drive `maybe_handle_http_proxy` directly through in-memory duplex
//! streams and real loopback upstreams, exercising auth, body handling, the
//! various error responses and the CONNECT/absolute-form relay paths.

#![cfg(feature = "proxy")]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream};
use tokio::net::TcpListener;

use aex::connection::context::{BoxReader, BoxWriter, Context};
use aex::connection::global::GlobalContext;
use aex::http::meta::HttpMetadata;
use aex::http::protocol::header::HeaderKey;
use aex::http::protocol::method::HttpMethod;
use aex::proxy::{ProxyAuthorizer, maybe_handle_http_proxy};

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn free_addr() -> SocketAddr {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
}

/// Build a proxy [`Context`] wired to two duplex streams. `client_tx` is what
/// the "client" writes (handler reads it); `client_rx` is what the client
/// reads (handler writes it). `with_reader` controls whether a request stream
/// is installed.
fn proxy_ctx(with_reader: bool) -> (Context, DuplexStream, DuplexStream) {
    let addr = free_addr();
    let global = Arc::new(GlobalContext::new(addr, None));
    let (client_tx, handler_rx) = tokio::io::duplex(1 << 16);
    let (handler_tx, client_rx) = tokio::io::duplex(1 << 16);
    let reader: Option<BoxReader> =
        with_reader.then(|| Box::new(BufReader::new(handler_rx)) as BoxReader);
    let writer: BoxWriter = Box::new(handler_tx);
    let ctx = Context::new(reader, Some(writer), global, addr);
    (ctx, client_tx, client_rx)
}

fn spawn_proxy(ctx: Context, authz: Option<ProxyAuthorizer>) -> tokio::task::JoinHandle<bool> {
    tokio::spawn(async move {
        let mut ctx = ctx;
        maybe_handle_http_proxy(&mut ctx, authz.as_ref()).await
    })
}

/// Read everything the handler sends, stopping at EOF or a short timeout.
async fn read_all(rx: &mut DuplexStream) -> String {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 2048];
    loop {
        match tokio::time::timeout(Duration::from_secs(3), rx.read(&mut tmp)).await {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => buf.extend_from_slice(&tmp[..n]),
            Ok(Err(_)) => break,
            Err(_) => break,
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

async fn read_exact_t(rx: &mut DuplexStream, buf: &mut [u8]) {
    tokio::time::timeout(Duration::from_secs(3), rx.read_exact(buf))
        .await
        .expect("timed out waiting for bytes")
        .expect("read failed");
}

async fn read_some_t(rx: &mut DuplexStream, buf: &mut [u8]) -> usize {
    tokio::time::timeout(Duration::from_secs(3), rx.read(buf))
        .await
        .expect("timed out waiting for bytes")
        .expect("read failed")
}

/// Origin server that echoes the exact request it received back as the body.
async fn spawn_http_upstream() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut tmp = [0u8; 2048];
                loop {
                    let n = match sock.read(&mut tmp).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    buf.extend_from_slice(&tmp[..n]);
                    if let Some(pos) = find_double_crlf(&buf) {
                        let head_end = pos + 4;
                        let cl = content_length(&buf[..head_end]);
                        if buf.len() >= head_end + cl {
                            break;
                        }
                    }
                }
                let body = std::str::from_utf8(&buf).unwrap_or("");
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    addr
}

/// One-shot byte echo upstream (used for CONNECT tunnels). It echoes the first
/// chunk it receives and then closes, so the tunnel's upstream read reaches EOF.
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

fn find_double_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn content_length(head: &[u8]) -> usize {
    let text = String::from_utf8_lossy(head);
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("Content-Length:") {
            if let Ok(n) = v.trim().parse::<usize>() {
                return n;
            }
        }
    }
    0
}

fn basic(user: &str, pass: &str) -> String {
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("{user}:{pass}"))
    )
}

fn alice() -> ProxyAuthorizer {
    Arc::new(|u: &str, p: &str| u == "alice" && p == "wonderland")
}

fn connect_meta(authority: &str) -> HttpMetadata {
    let mut meta = HttpMetadata::new();
    meta.method = HttpMethod::CONNECT;
    meta.path = authority.to_string();
    meta
}

fn get_meta(path: &str) -> HttpMetadata {
    let mut meta = HttpMetadata::new();
    meta.method = HttpMethod::GET;
    meta.path = path.to_string();
    meta
}

// ---------------------------------------------------------------------------
// dispatch
// ---------------------------------------------------------------------------

#[tokio::test]
async fn no_metadata_returns_false() {
    let (mut ctx, _tx, _rx) = proxy_ctx(true);
    let handled = maybe_handle_http_proxy(&mut ctx, None).await;
    assert!(!handled);
}

#[tokio::test]
async fn origin_form_returns_false() {
    let (mut ctx, _tx, _rx) = proxy_ctx(true);
    ctx.local.set_value(get_meta("/plain/path"));
    assert!(!maybe_handle_http_proxy(&mut ctx, None).await);
}

#[tokio::test]
async fn https_absolute_form_returns_502() {
    let (mut ctx, _tx, mut rx) = proxy_ctx(true);
    ctx.local.set_value(get_meta("https://example.com/path"));
    assert!(maybe_handle_http_proxy(&mut ctx, None).await);
    let resp = read_all(&mut rx).await;
    assert!(resp.starts_with("HTTP/1.1 502"), "got {resp}");
}

// ---------------------------------------------------------------------------
// authorizer / Basic auth
// ---------------------------------------------------------------------------

#[tokio::test]
async fn connect_auth_required_missing_creds_407() {
    let (mut ctx, _tx, mut rx) = proxy_ctx(true);
    ctx.local.set_value(connect_meta("127.0.0.1:1"));
    let authz = Some(alice());
    assert!(maybe_handle_http_proxy(&mut ctx, authz.as_ref()).await);
    let resp = read_all(&mut rx).await;
    assert!(resp.starts_with("HTTP/1.1 407"), "got {resp}");
}

#[tokio::test]
async fn connect_wrong_creds_407() {
    let (mut ctx, _tx, mut rx) = proxy_ctx(true);
    let mut meta = connect_meta("127.0.0.1:1");
    meta.headers
        .insert(HeaderKey::from("Proxy-Authorization"), basic("bob", "nope"));
    ctx.local.set_value(meta);
    let authz = Some(alice());
    assert!(maybe_handle_http_proxy(&mut ctx, authz.as_ref()).await);
    let resp = read_all(&mut rx).await;
    assert!(resp.starts_with("HTTP/1.1 407"), "got {resp}");
}

#[tokio::test]
async fn connect_invalid_base64_creds_407() {
    let (mut ctx, _tx, mut rx) = proxy_ctx(true);
    let mut meta = connect_meta("127.0.0.1:1");
    meta.headers.insert(
        HeaderKey::from("Proxy-Authorization"),
        "Basic !!!notbase64!!!",
    );
    ctx.local.set_value(meta);
    let authz = Some(alice());
    assert!(maybe_handle_http_proxy(&mut ctx, authz.as_ref()).await);
    let resp = read_all(&mut rx).await;
    assert!(resp.starts_with("HTTP/1.1 407"), "got {resp}");
}

#[tokio::test]
async fn connect_creds_without_colon_407() {
    let (mut ctx, _tx, mut rx) = proxy_ctx(true);
    let mut meta = connect_meta("127.0.0.1:1");
    let encoded = base64::engine::general_purpose::STANDARD.encode("nocolon");
    meta.headers.insert(
        HeaderKey::from("Proxy-Authorization"),
        format!("Basic {encoded}"),
    );
    ctx.local.set_value(meta);
    let authz = Some(alice());
    assert!(maybe_handle_http_proxy(&mut ctx, authz.as_ref()).await);
    let resp = read_all(&mut rx).await;
    assert!(resp.starts_with("HTTP/1.1 407"), "got {resp}");
}

#[tokio::test]
async fn connect_non_basic_scheme_407() {
    let (mut ctx, _tx, mut rx) = proxy_ctx(true);
    let mut meta = connect_meta("127.0.0.1:1");
    meta.headers
        .insert(HeaderKey::from("Proxy-Authorization"), "Bearer token");
    ctx.local.set_value(meta);
    let authz = Some(alice());
    assert!(maybe_handle_http_proxy(&mut ctx, authz.as_ref()).await);
    let resp = read_all(&mut rx).await;
    assert!(resp.starts_with("HTTP/1.1 407"), "got {resp}");
}

#[tokio::test]
async fn absolute_form_wrong_creds_407() {
    let (mut ctx, _tx, mut rx) = proxy_ctx(true);
    let mut meta = get_meta("http://127.0.0.1:1/x");
    meta.headers
        .insert(HeaderKey::from("Proxy-Authorization"), basic("bob", "nope"));
    ctx.local.set_value(meta);
    let authz = Some(alice());
    assert!(maybe_handle_http_proxy(&mut ctx, authz.as_ref()).await);
    let resp = read_all(&mut rx).await;
    assert!(resp.starts_with("HTTP/1.1 407"), "got {resp}");
}

// ---------------------------------------------------------------------------
// CONNECT errors
// ---------------------------------------------------------------------------

#[tokio::test]
async fn connect_malformed_authority_400() {
    let (mut ctx, _tx, mut rx) = proxy_ctx(true);
    ctx.local.set_value(connect_meta("127.0.0.1:notaport"));
    assert!(maybe_handle_http_proxy(&mut ctx, None).await);
    let resp = read_all(&mut rx).await;
    assert!(resp.starts_with("HTTP/1.1 400"), "got {resp}");
}

#[tokio::test]
async fn connect_refused_502() {
    let dead = free_addr();
    let (mut ctx, _tx, mut rx) = proxy_ctx(true);
    ctx.local.set_value(connect_meta(&dead.to_string()));
    assert!(maybe_handle_http_proxy(&mut ctx, None).await);
    let resp = read_all(&mut rx).await;
    assert!(resp.starts_with("HTTP/1.1 502"), "got {resp}");
}

// ---------------------------------------------------------------------------
// CONNECT success + tunnel
// ---------------------------------------------------------------------------

#[tokio::test]
async fn connect_success_tunnels_raw_bytes() {
    let upstream = spawn_echo_upstream().await;
    let (ctx, mut tx, mut rx) = proxy_ctx(true);
    let mut meta = connect_meta(&upstream.to_string());
    meta.headers.insert(HeaderKey::Host, upstream.to_string());
    let mut ctx = ctx;
    ctx.local.set_value(meta);

    let join = spawn_proxy(ctx, None);
    let mut head = [0u8; 64];
    let n = read_some_t(&mut rx, &mut head).await;
    assert!(
        String::from_utf8_lossy(&head[..n]).contains("200"),
        "got {:?}",
        &head[..n]
    );

    tx.write_all(b"ping").await.unwrap();
    let mut echo = [0u8; 4];
    read_exact_t(&mut rx, &mut echo).await;
    assert_eq!(&echo, b"ping");

    drop(tx);
    drop(rx);
    tokio::time::timeout(Duration::from_secs(3), join)
        .await
        .expect("connect session should finish")
        .unwrap();
}

#[tokio::test]
async fn connect_success_with_bracketed_ipv4_authority() {
    let upstream = spawn_echo_upstream().await;
    let authority = format!("[127.0.0.1]:{}", upstream.port());
    let (mut ctx, mut tx, mut rx) = proxy_ctx(true);
    ctx.local.set_value(connect_meta(&authority));

    let join = spawn_proxy(ctx, None);
    let mut head = [0u8; 64];
    let n = read_some_t(&mut rx, &mut head).await;
    assert!(
        String::from_utf8_lossy(&head[..n]).contains("200"),
        "got {:?}",
        &head[..n]
    );

    tx.write_all(b"abc").await.unwrap();
    let mut echo = [0u8; 3];
    read_exact_t(&mut rx, &mut echo).await;
    assert_eq!(&echo, b"abc");

    drop(tx);
    drop(rx);
    tokio::time::timeout(Duration::from_secs(3), join)
        .await
        .expect("connect session should finish")
        .unwrap();
}

#[tokio::test]
async fn connect_success_with_auth_tunnels() {
    let upstream = spawn_echo_upstream().await;
    let (ctx, mut tx, mut rx) = proxy_ctx(true);
    let mut meta = connect_meta(&upstream.to_string());
    meta.headers.insert(
        HeaderKey::from("Proxy-Authorization"),
        basic("alice", "wonderland"),
    );
    let mut ctx = ctx;
    ctx.local.set_value(meta);

    let join = spawn_proxy(ctx, Some(alice()));
    let mut head = [0u8; 64];
    let n = read_some_t(&mut rx, &mut head).await;
    assert!(
        String::from_utf8_lossy(&head[..n]).contains("200"),
        "got {:?}",
        &head[..n]
    );

    tx.write_all(b"auth-ok").await.unwrap();
    let mut echo = [0u8; 7];
    read_exact_t(&mut rx, &mut echo).await;
    assert_eq!(&echo, b"auth-ok");

    drop(tx);
    drop(rx);
    tokio::time::timeout(Duration::from_secs(3), join)
        .await
        .expect("connect session should finish")
        .unwrap();
}

#[tokio::test]
async fn connect_without_reader_returns_after_200() {
    let upstream = spawn_echo_upstream().await;
    let (ctx, _tx, mut rx) = proxy_ctx(false);
    let meta = connect_meta(&upstream.to_string());
    let mut ctx = ctx;
    ctx.local.set_value(meta);

    let join = spawn_proxy(ctx, None);
    let mut head = [0u8; 64];
    let n = read_some_t(&mut rx, &mut head).await;
    assert!(
        String::from_utf8_lossy(&head[..n]).contains("200"),
        "got {:?}",
        &head[..n]
    );
    tokio::time::timeout(Duration::from_secs(3), join)
        .await
        .expect("session should finish without piping")
        .unwrap();
}

// ---------------------------------------------------------------------------
// absolute-form errors
// ---------------------------------------------------------------------------

#[tokio::test]
async fn absolute_form_chunked_501() {
    let (mut ctx, _tx, mut rx) = proxy_ctx(true);
    let mut meta = get_meta("http://127.0.0.1:1/x");
    meta.is_chunked = true;
    ctx.local.set_value(meta);
    assert!(maybe_handle_http_proxy(&mut ctx, None).await);
    let resp = read_all(&mut rx).await;
    assert!(resp.starts_with("HTTP/1.1 501"), "got {resp}");
}

#[tokio::test]
async fn absolute_form_body_too_large_413() {
    let (mut ctx, _tx, mut rx) = proxy_ctx(true);
    let mut meta = get_meta("http://127.0.0.1:1/x");
    meta.headers
        .insert(HeaderKey::ContentLength, "9000000".to_string());
    ctx.local.set_value(meta);
    assert!(maybe_handle_http_proxy(&mut ctx, None).await);
    let resp = read_all(&mut rx).await;
    assert!(resp.starts_with("HTTP/1.1 413"), "got {resp}");
}

#[tokio::test]
async fn absolute_form_missing_reader_400() {
    let (mut ctx, _tx, mut rx) = proxy_ctx(false);
    let mut meta = get_meta("http://127.0.0.1:1/x");
    meta.headers
        .insert(HeaderKey::ContentLength, "10".to_string());
    ctx.local.set_value(meta);
    assert!(maybe_handle_http_proxy(&mut ctx, None).await);
    let resp = read_all(&mut rx).await;
    assert!(resp.starts_with("HTTP/1.1 400"), "got {resp}");
}

#[tokio::test]
async fn absolute_form_truncated_body_400() {
    let (mut ctx, mut tx, mut rx) = proxy_ctx(true);
    let mut meta = get_meta("http://127.0.0.1:1/x");
    meta.headers
        .insert(HeaderKey::ContentLength, "100".to_string());
    ctx.local.set_value(meta);
    tx.write_all(b"short").await.unwrap();
    drop(tx);

    assert!(maybe_handle_http_proxy(&mut ctx, None).await);
    let resp = read_all(&mut rx).await;
    assert!(resp.starts_with("HTTP/1.1 400"), "got {resp}");
}

#[tokio::test]
async fn absolute_form_upstream_refused_502() {
    let dead = free_addr();
    let (mut ctx, _tx, mut rx) = proxy_ctx(true);
    ctx.local.set_value(get_meta(&format!("http://{dead}/x")));
    assert!(maybe_handle_http_proxy(&mut ctx, None).await);
    let resp = read_all(&mut rx).await;
    assert!(resp.starts_with("HTTP/1.1 502"), "got {resp}");
}

#[tokio::test]
async fn absolute_form_default_port_unreachable_502() {
    let (mut ctx, _tx, mut rx) = proxy_ctx(true);
    ctx.local
        .set_value(get_meta("http://nonexistent.invalid/x"));
    assert!(maybe_handle_http_proxy(&mut ctx, None).await);
    let resp = read_all(&mut rx).await;
    assert!(resp.starts_with("HTTP/1.1 502"), "got {resp}");
}

// ---------------------------------------------------------------------------
// absolute-form success
// ---------------------------------------------------------------------------

#[tokio::test]
async fn absolute_form_get_relays_and_strips_hop_by_hop() {
    let upstream = spawn_http_upstream().await;
    let (mut ctx, _tx, mut rx) = proxy_ctx(true);
    let mut meta = get_meta(&format!("http://user:pw@{upstream}/a/b"));
    meta.headers.insert(HeaderKey::Host, "ignored".to_string());
    meta.headers
        .insert(HeaderKey::Connection, "keep-alive".to_string());
    meta.headers
        .insert(HeaderKey::from("Proxy-Authorization"), "Basic xxx");
    meta.headers
        .insert(HeaderKey::from("X-Custom"), "keepme".to_string());
    ctx.local.set_value(meta);

    assert!(maybe_handle_http_proxy(&mut ctx, None).await);
    let resp = read_all(&mut rx).await;
    assert!(resp.contains("200 OK"), "got {resp}");
    assert!(resp.contains("GET /a/b HTTP/1.1"), "got {resp}");
    assert!(resp.contains("X-Custom: keepme"), "got {resp}");
    assert!(!resp.contains("Proxy-Authorization"), "got {resp}");
    assert!(!resp.contains("keep-alive"), "got {resp}");
}

#[tokio::test]
async fn absolute_form_post_body_forwarded() {
    let upstream = spawn_http_upstream().await;
    let (mut ctx, mut tx, mut rx) = proxy_ctx(true);
    let body = b"hello=world&x=1";
    let mut meta = HttpMetadata::new();
    meta.method = HttpMethod::POST;
    meta.path = format!("http://{upstream}/submit");
    meta.headers
        .insert(HeaderKey::ContentLength, body.len().to_string());
    ctx.local.set_value(meta);
    tx.write_all(body).await.unwrap();

    assert!(maybe_handle_http_proxy(&mut ctx, None).await);
    let resp = read_all(&mut rx).await;
    assert!(resp.contains("POST /submit HTTP/1.1"), "got {resp}");
    assert!(resp.contains("hello=world&x=1"), "got {resp}");
}

#[tokio::test]
async fn absolute_form_with_auth_success() {
    let upstream = spawn_http_upstream().await;
    let (mut ctx, _tx, mut rx) = proxy_ctx(true);
    let mut meta = get_meta(&format!("http://{upstream}/authed"));
    meta.headers.insert(
        HeaderKey::from("Proxy-Authorization"),
        basic("alice", "wonderland"),
    );
    ctx.local.set_value(meta);
    let authz = Some(alice());

    assert!(maybe_handle_http_proxy(&mut ctx, authz.as_ref()).await);
    let resp = read_all(&mut rx).await;
    assert!(resp.contains("200 OK"), "got {resp}");
    assert!(resp.contains("GET /authed HTTP/1.1"), "got {resp}");
}

#[tokio::test]
async fn absolute_form_each_method_is_rewritten() {
    let upstream = spawn_http_upstream().await;
    let cases: &[(HttpMethod, &str)] = &[
        (HttpMethod::GET, "GET"),
        (HttpMethod::HEAD, "HEAD"),
        (HttpMethod::POST, "POST"),
        (HttpMethod::PUT, "PUT"),
        (HttpMethod::DELETE, "DELETE"),
        (HttpMethod::OPTIONS, "OPTIONS"),
        (HttpMethod::TRACE, "TRACE"),
        (HttpMethod::PATCH, "PATCH"),
        // Non-standard methods fall through to the default "GET".
        (HttpMethod::PROPFIND, "GET"),
    ];

    for (method, expected) in cases {
        let (mut ctx, _tx, mut rx) = proxy_ctx(true);
        let mut meta = get_meta(&format!("http://{upstream}/m"));
        meta.method = *method;
        ctx.local.set_value(meta);

        assert!(maybe_handle_http_proxy(&mut ctx, None).await);
        let resp = read_all(&mut rx).await;
        assert!(
            resp.contains(&format!("{expected} /m HTTP/1.1")),
            "method {method:?}: got {resp}"
        );
    }
}
