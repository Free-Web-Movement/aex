//! Integration tests for the built-in protocol structs: constructors, builder
//! methods and the uniform [`Protocol`] trait surface.
//!
//! Covers `Tcp`, `Udp`, `Websocket`, `Http1`, `Http2` (and `Socks` when the
//! `proxy` feature is enabled). Every test uses only the public API and local
//! loopback sockets.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use bytes::Bytes;
use futures::future::BoxFuture;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use aex::connection::context::{BoxReader, BoxWriter, Context};
use aex::connection::global::GlobalContext;
use aex::handler::{CommandHandler, Outcome};
use aex::http::meta::HttpMetadata;
use aex::http::protocol::method::HttpMethod;
use aex::http::protocol::status::StatusCode;
use aex::http::router::Router as HttpRouter;
use aex::http::types::Executor;
use aex::protocol::{Protocol, Transport};
use aex::protocols::http1::{Http1, HttpHandler};
use aex::protocols::http2::{Http2, Http2Handler};
use aex::protocols::tcp::{TCPHandler, Tcp, from_handler, router_handler};
use aex::protocols::udp::{UDPHandler, Udp};
use aex::protocols::websocket::Websocket;

// ---------------------------------------------------------------------------
// Shared scaffolding
// ---------------------------------------------------------------------------

fn globals() -> Arc<GlobalContext> {
    Arc::new(GlobalContext::new("127.0.0.1:0".parse().unwrap(), None))
}

fn empty_ctx() -> Context {
    let addr: SocketAddr = "127.0.0.1:9000".parse().unwrap();
    Context::new(None, None, globals(), addr)
}

/// A real loopback TCP pair: `client` sends, and the returned `Context` carries
/// the server-side reader/writer halves so `handle`/`process` can run.
async fn tcp_ctx_pair() -> (tokio::net::TcpStream, Context) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let client = tokio::net::TcpStream::connect(addr).await.unwrap();
    let (server, peer) = listener.accept().await.unwrap();
    let (r, w) = server.into_split();
    let reader: BoxReader = Box::new(tokio::io::BufReader::new(r));
    let writer: BoxWriter = Box::new(w);
    let ctx = Context::new(Some(reader), Some(writer), globals(), peer);
    (client, ctx)
}

fn counting_tcp_handler(calls: Arc<AtomicUsize>) -> TCPHandler {
    Arc::new(move |_ctx: Context| {
        calls.fetch_add(1, Ordering::SeqCst);
        tokio::spawn(async {})
    })
}

fn counting_udp_handler(calls: Arc<AtomicUsize>) -> UDPHandler {
    Arc::new(move |_ctx: Context| {
        calls.fetch_add(1, Ordering::SeqCst);
        tokio::spawn(async {})
    })
}

fn executor(calls: Arc<AtomicUsize>) -> Arc<Executor> {
    Arc::new(move |_ctx: &mut Context| -> BoxFuture<'_, bool> {
        let calls = calls.clone();
        Box::pin(async move {
            calls.fetch_add(1, Ordering::SeqCst);
            true
        })
    })
}

fn noop_cmd() -> CommandHandler<Bytes> {
    Arc::new(
        |_ctx: &mut Context, _payload: Bytes| -> BoxFuture<'_, Outcome> {
            Box::pin(async { Outcome::Done })
        },
    )
}

fn http_handler(marker: &'static str) -> HttpHandler {
    Arc::new(move |ctx: &mut Context| {
        ctx.status(StatusCode::Ok);
        ctx.text(marker);
        Box::pin(async { true })
    })
}

fn h2_handler(marker: &'static str) -> Http2Handler {
    Arc::new(move |ctx: &mut Context| {
        ctx.status(StatusCode::Ok);
        ctx.text(marker);
        Box::pin(async { true })
    })
}

// ---------------------------------------------------------------------------
// Tcp
// ---------------------------------------------------------------------------

#[test]
fn tcp_new_defaults_and_accessors() {
    let tcp = Tcp::new(None);
    let p: &dyn Protocol = &tcp;
    assert_eq!(p.name(), "tcp");
    assert_eq!(p.transport(), Transport::Stream);
    assert!(p.is_default());
    assert!(!p.buffered_writer());
    assert!(p.middleware().is_empty());
    assert!(p.sub_handlers().is_empty());
    assert!(p.commands().is_empty());
    assert!(p.detector().is_none());
}

#[tokio::test]
async fn tcp_handle_without_handler_returns_done() {
    let tcp = Tcp::new(None);
    let mut ctx = empty_ctx();
    assert_eq!(tcp.handle(&mut ctx).await, Outcome::Done);
}

#[tokio::test]
async fn tcp_handle_with_handler_invokes_closure() {
    let calls = Arc::new(AtomicUsize::new(0));
    let tcp = Tcp::new(Some(counting_tcp_handler(calls.clone())));
    let mut ctx = empty_ctx();
    assert_eq!(tcp.handle(&mut ctx).await, Outcome::Done);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn tcp_process_delegates_to_handle_and_builders_accumulate() {
    let calls = Arc::new(AtomicUsize::new(0));
    let ws: Arc<dyn Protocol> = Arc::new(Websocket::new(None));
    let tcp = Tcp::new(None)
        .with_middleware(executor(calls.clone()))
        .sub(ws)
        .with_command("ping", noop_cmd());

    let p: &dyn Protocol = &tcp;
    assert_eq!(p.middleware().len(), 1);
    assert_eq!(p.sub_handlers().len(), 1);
    assert_eq!(p.commands().len(), 1);

    let mut ctx = empty_ctx();
    assert_eq!(tcp.process(&mut ctx).await, Outcome::Done);

    let shared: Arc<dyn Protocol> = tcp.shared();
    assert_eq!(shared.name(), "tcp");
}

#[tokio::test]
async fn tcp_from_handler_some_and_none_branches() {
    let calls = Arc::new(AtomicUsize::new(0));
    let some = from_handler("custom", Some(counting_tcp_handler(calls.clone())));
    let mut ctx = empty_ctx();
    assert_eq!(some.process(&mut ctx).await, Outcome::Done);
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let none = from_handler("custom-none", None::<TCPHandler>);
    let mut ctx2 = empty_ctx();
    assert_eq!(none.process(&mut ctx2).await, Outcome::Done);
}

#[tokio::test]
async fn tcp_router_handler_runs_router_on_context() {
    let router = Arc::new(aex::protocols::tcp::router::Router::new());
    let handler = router_handler(router);
    // No reader installed: the router returns immediately without panicking.
    let join = handler(empty_ctx());
    let _ = join.await;
}

// ---------------------------------------------------------------------------
// Udp
// ---------------------------------------------------------------------------

#[test]
fn udp_new_defaults_and_accessors() {
    let udp = Udp::new(None);
    let p: &dyn Protocol = &udp;
    assert_eq!(p.name(), "udp");
    assert_eq!(p.transport(), Transport::Datagram);
    assert!(p.is_default());
    assert!(!p.buffered_writer());
    assert!(p.middleware().is_empty());
    assert!(p.sub_handlers().is_empty());
    assert!(p.commands().is_empty());
    assert!(p.detector().is_none());
}

#[tokio::test]
async fn udp_handle_without_handler_returns_done() {
    let udp = Udp::new(None);
    let mut ctx = empty_ctx();
    assert_eq!(udp.handle(&mut ctx).await, Outcome::Done);
}

#[tokio::test]
async fn udp_handle_with_handler_invokes_closure() {
    let calls = Arc::new(AtomicUsize::new(0));
    let udp = Udp::new(Some(counting_udp_handler(calls.clone())));
    let mut ctx = empty_ctx();
    assert_eq!(udp.handle(&mut ctx).await, Outcome::Done);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn udp_process_builders_and_shared() {
    let calls = Arc::new(AtomicUsize::new(0));
    let child: Arc<dyn Protocol> = Arc::new(Udp::new(None));
    let udp = Udp::new(None)
        .with_middleware(executor(calls.clone()))
        .sub(child)
        .with_command("datagram", noop_cmd());

    let p: &dyn Protocol = &udp;
    assert_eq!(p.middleware().len(), 1);
    assert_eq!(p.sub_handlers().len(), 1);
    assert_eq!(p.commands().len(), 1);

    let mut ctx = empty_ctx();
    assert_eq!(udp.process(&mut ctx).await, Outcome::Done);
    let shared: Arc<dyn Protocol> = udp.shared();
    assert_eq!(shared.name(), "udp");
}

// ---------------------------------------------------------------------------
// Websocket (sub-protocol)
// ---------------------------------------------------------------------------

#[test]
fn websocket_new_defaults_and_accessors() {
    let ws = Websocket::new(None);
    let p: &dyn Protocol = &ws;
    assert_eq!(p.name(), "websocket");
    assert!(p.router().is_none());
    assert_eq!(p.transport(), Transport::Stream);
    assert!(p.middleware().is_empty());
    assert!(p.sub_handlers().is_empty());
    assert!(p.commands().is_empty());
    assert!(p.detector().is_none());
}

#[test]
fn websocket_with_router_middleware_and_command() {
    let router = Arc::new(HttpRouter::default());
    let ws = Websocket::new(Some(router))
        .with_middleware(executor(Arc::new(AtomicUsize::new(0))))
        .with_command("frame", noop_cmd());
    let p: &dyn Protocol = &ws;
    assert!(p.router().is_some());
    assert_eq!(p.middleware().len(), 1);
    assert_eq!(p.commands().len(), 1);
}

#[tokio::test]
async fn websocket_handle_without_router_sends_failure() {
    let ws = Websocket::new(None);
    let (_client, mut ctx) = tcp_ctx_pair().await;
    assert_eq!(ws.handle(&mut ctx).await, Outcome::Done);
}

#[tokio::test]
async fn websocket_handle_router_miss_and_hit() {
    // Router present but no matching route -> failure branch.
    let ws = Websocket::new(Some(Arc::new(HttpRouter::default())));
    let (_client, mut ctx) = tcp_ctx_pair().await;
    ctx.local.set_value(HttpMetadata {
        path: "/missing".to_string(),
        method: HttpMethod::GET,
        ..Default::default()
    });
    assert_eq!(ws.handle(&mut ctx).await, Outcome::Done);

    // Matching route -> send_response branch, exercised through `process`.
    let mut router = HttpRouter::default();
    router.get("/", |_ctx: &mut Context| true);
    let ws = Websocket::new(Some(Arc::new(router)));
    let (_client, mut ctx) = tcp_ctx_pair().await;
    ctx.local.set_value(HttpMetadata {
        path: "/".to_string(),
        method: HttpMethod::GET,
        ..Default::default()
    });
    assert_eq!(ws.process(&mut ctx).await, Outcome::Done);
}

// ---------------------------------------------------------------------------
// Http1
// ---------------------------------------------------------------------------

#[test]
fn http1_new_defaults_and_accessors() {
    let http = Http1::new(None, None);
    let p: &dyn Protocol = &http;
    assert_eq!(p.name(), "http");
    assert!(p.detector().is_some());
    assert!(p.router().is_none());
    assert_eq!(p.sub_handlers().len(), 1);
    assert_eq!(p.sub_handlers()[0].name(), "websocket");
    assert!(p.buffered_writer());
    assert_eq!(p.transport(), Transport::Stream);
    assert!(p.middleware().is_empty());
    assert!(p.commands().is_empty());
}

#[test]
fn http1_with_websocket_replaces_default_sub_handler() {
    let http = Http1::new(None, None);
    let replacement: Arc<dyn Protocol> = Arc::new(Websocket::new(None));
    let http = http.with_websocket(replacement);
    assert_eq!(http.sub_handlers().len(), 1);
    assert_eq!(http.sub_handlers()[0].name(), "websocket");

    let http = http.with_middleware(executor(Arc::new(AtomicUsize::new(0))));
    assert_eq!(http.middleware().len(), 1);
    let http = http.with_command("req", noop_cmd());
    assert_eq!(http.commands().len(), 1);
    let _arc: Arc<Http1> = http.shared();
}

#[tokio::test]
async fn http1_handle_parse_failure_sends_failure() {
    let (mut client, mut ctx) = tcp_ctx_pair().await;
    client.shutdown().await.ok();
    drop(client);
    let http = Http1::new(None, None);
    assert_eq!(http.handle(&mut ctx).await, Outcome::Done);
}

#[tokio::test]
async fn http1_handle_routes_to_router() {
    let (mut client, mut ctx) = tcp_ctx_pair().await;
    let mut router = HttpRouter::default();
    router.get("/", |ctx: &mut Context| {
        ctx.text("hello");
        true
    });
    let http = Http1::new(Some(Arc::new(router)), None);
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n")
        .await
        .unwrap();
    assert_eq!(http.handle(&mut ctx).await, Outcome::Done);

    let mut buf = vec![0u8; 128];
    let n = client.read(&mut buf).await.unwrap();
    assert!(buf[..n].starts_with(b"HTTP/1.1 200"), "got {:?}", &buf[..n]);
}

#[tokio::test]
async fn http1_handle_explicit_handler_takes_precedence() {
    let (mut client, mut ctx) = tcp_ctx_pair().await;
    let http = Http1::new(None, Some(http_handler("handled")));
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n")
        .await
        .unwrap();
    assert_eq!(http.process(&mut ctx).await, Outcome::Done);
    let mut buf = vec![0u8; 128];
    let n = client.read(&mut buf).await.unwrap();
    assert!(String::from_utf8_lossy(&buf[..n]).contains("handled"));
}

#[tokio::test]
async fn http1_handle_router_miss_sends_failure() {
    let (mut client, mut ctx) = tcp_ctx_pair().await;
    let http = Http1::new(None, None);
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n")
        .await
        .unwrap();
    assert_eq!(http.handle(&mut ctx).await, Outcome::Done);
    let mut buf = vec![0u8; 128];
    let _ = client.read(&mut buf).await.unwrap();
}

#[tokio::test]
async fn http1_handle_websocket_upgrade_hands_off() {
    let (mut client, mut ctx) = tcp_ctx_pair().await;
    let http = Http1::new(None, None);
    client
        .write_all(
            b"GET /ws HTTP/1.1\r\n\
              Host: x\r\n\
              Upgrade: websocket\r\n\
              Connection: Upgrade\r\n\
              Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
              Sec-WebSocket-Version: 13\r\n\r\n",
        )
        .await
        .unwrap();
    assert_eq!(
        http.process(&mut ctx).await,
        Outcome::Handoff("websocket".to_string())
    );
}

#[cfg(feature = "proxy")]
mod http1_proxy {
    use super::*;

    #[test]
    fn http1_proxy_flag_toggle() {
        let off = Http1::new(None, None);
        assert!(!off.proxy_enabled());
        assert!(Http1::new(None, None).with_proxy(None).proxy_enabled());
        let auth: aex::proxy::ProxyAuthorizer = Arc::new(|_u: &str, _p: &str| true);
        assert!(
            Http1::new(None, None)
                .with_proxy(Some(auth))
                .proxy_enabled()
        );
    }

    #[tokio::test]
    async fn http1_handle_https_absolute_form_is_proxied() {
        let (mut client, mut ctx) = tcp_ctx_pair().await;
        let http = Http1::new(None, None).with_proxy(None);
        client
            .write_all(b"GET https://example.com/ HTTP/1.1\r\nHost: example.com\r\n\r\n")
            .await
            .unwrap();
        assert_eq!(http.handle(&mut ctx).await, Outcome::Done);
        let mut buf = vec![0u8; 256];
        let n = client.read(&mut buf).await.unwrap();
        assert!(
            String::from_utf8_lossy(&buf[..n]).contains("502"),
            "got {:?}",
            &buf[..n]
        );
    }
}

// ---------------------------------------------------------------------------
// Http2
// ---------------------------------------------------------------------------

#[test]
fn http2_new_defaults_and_builders() {
    let h2 = Http2::new(None, None);
    let p: &dyn Protocol = &h2;
    assert_eq!(p.name(), "http2");
    assert!(p.detector().is_some());
    assert_eq!(p.transport(), Transport::Stream);
    assert!(!p.is_default());
    assert!(!p.buffered_writer());
    assert!(p.middleware().is_empty());
    assert!(p.sub_handlers().is_empty());
    assert!(p.commands().is_empty());

    let h2 = h2.with_command("stream", noop_cmd());
    assert_eq!(h2.commands().len(), 1);
    let _arc: Arc<Http2> = h2.shared();
}

#[tokio::test]
async fn http2_handle_bad_handshake_returns_done() {
    let (mut client, mut ctx) = tcp_ctx_pair().await;
    let h2 = Http2::new(None, Some(h2_handler("hi")));
    client
        .write_all(b"not the h2 preface at all\r\n\r\n")
        .await
        .unwrap();
    client.flush().await.unwrap();
    let outcome = tokio::time::timeout(Duration::from_secs(3), h2.handle(&mut ctx))
        .await
        .expect("handshake should fail fast");
    assert_eq!(outcome, Outcome::Done);
}

#[tokio::test]
async fn http2_handle_serves_a_request() {
    let (client, mut ctx) = tcp_ctx_pair().await;
    let h2 = Http2::new(None, Some(h2_handler("hello-h2")));
    let server = tokio::spawn(async move { h2.handle(&mut ctx).await });

    let (mut send_request, conn) = h2::client::handshake(client).await.unwrap();
    tokio::spawn(async move {
        let _ = conn.await;
    });

    let request = http::Request::builder()
        .method("GET")
        .uri("http://localhost/")
        .body(())
        .unwrap();
    let (response, _) = send_request.send_request(request, false).unwrap();
    let resp = tokio::time::timeout(Duration::from_secs(5), response)
        .await
        .expect("response timeout")
        .unwrap();
    assert_eq!(resp.status(), http::StatusCode::OK);

    let mut body = resp.into_body();
    let mut data = Vec::new();
    while let Some(Ok(chunk)) = body.data().await {
        data.extend_from_slice(&chunk);
    }
    assert_eq!(String::from_utf8_lossy(&data), "hello-h2");

    server.abort();
}

// ---------------------------------------------------------------------------
// Socks (proxy feature)
// ---------------------------------------------------------------------------

#[cfg(feature = "proxy")]
mod socks_proto {
    use super::*;
    use aex::protocols::socks::{Socks, protocol as socks_protocol};

    #[test]
    fn socks_new_defaults_and_builders() {
        let socks = Socks::new(None);
        let p: &dyn Protocol = &socks;
        assert_eq!(p.name(), "socks");
        assert_eq!(p.transport(), Transport::Stream);
        assert!(p.detector().is_some());
        assert!(!p.is_default());
        assert!(!p.buffered_writer());
        assert!(p.sub_handlers().is_empty());
        assert!(p.commands().is_empty());

        let auth: aex::proxy::ProxyAuthorizer = Arc::new(|_u: &str, _p: &str| true);
        let socks = Socks::new(Some(auth)).with_command("socks", noop_cmd());
        assert_eq!(socks.commands().len(), 1);
    }

    #[tokio::test]
    async fn socks_handle_and_process() {
        let socks = Socks::new(None);
        let (_client, mut ctx) = tcp_ctx_pair().await;
        assert_eq!(socks.handle(&mut ctx).await, Outcome::Done);

        let socks = Socks::new(None);
        let (_client, mut ctx) = tcp_ctx_pair().await;
        assert_eq!(socks.process(&mut ctx).await, Outcome::Done);
    }

    #[test]
    fn socks_shared_and_protocol_builder() {
        let _arc: Arc<Socks> = Socks::new(None).shared();
        let handler = socks_protocol(None);
        assert_eq!(handler.name(), "socks");
        assert!(handler.detector().is_some());
    }
}
