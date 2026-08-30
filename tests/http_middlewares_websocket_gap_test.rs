use aex::{
    connection::{
        context::{BoxReader, BoxWriter, Context},
        global::GlobalContext,
    },
    http::{
        meta::HttpMetadata,
        middlewares::websocket::{WebSocket, WsSenderList},
        protocol::{header::{HeaderKey, Headers}, method::HttpMethod},
        websocket::{WSCodec, WSFrame},
    },
};
use std::{net::SocketAddr, sync::Arc};
use tokio::io::duplex;

fn addr(port: u16) -> SocketAddr {
    format!("127.0.0.1:{}", port).parse().unwrap()
}

#[test]
fn check_false_when_upgrade_missing() {
    let mut headers = Headers::new();
    headers.insert(HeaderKey::Connection, "Upgrade".to_string());
    assert!(!WebSocket::check(HttpMethod::GET, &headers));
}

#[test]
fn check_false_when_connection_missing() {
    let mut headers = Headers::new();
    headers.insert(HeaderKey::Upgrade, "websocket".to_string());
    assert!(!WebSocket::check(HttpMethod::GET, &headers));
}

#[test]
fn check_false_when_upgrade_not_websocket() {
    let mut headers = Headers::new();
    headers.insert(HeaderKey::Upgrade, "h2c".to_string());
    headers.insert(HeaderKey::Connection, "Upgrade".to_string());
    assert!(!WebSocket::check(HttpMethod::GET, &headers));
}

#[test]
fn check_accepts_case_insensitive_upgrade_and_connection_list() {
    let mut headers = Headers::new();
    headers.insert(HeaderKey::Upgrade, "WebSocket".to_string());
    headers.insert(HeaderKey::Connection, "keep-alive, Upgrade".to_string());
    assert!(WebSocket::check(HttpMethod::GET, &headers));
}

#[tokio::test]
async fn handshake_writes_101_with_rfc6455_accept_key() {
    use tokio::io::AsyncReadExt;
    let (mut client, mut server) = duplex(1024);
    let headers = Headers::new().with(
        HeaderKey::SecWebSocketKey,
        "dGhlIHNhbXBsZSBub25jZQ==",
    );
    WebSocket::handshake(&mut server, &headers).await.unwrap();

    let mut buf = [0u8; 512];
    let n = client.read(&mut buf).await.unwrap();
    let response = String::from_utf8_lossy(&buf[..n]).to_string();
    assert!(response.starts_with("HTTP/1.1 101 Switching Protocols"));
    assert!(response.contains("Upgrade: websocket"));
    assert!(response.contains("Connection: Upgrade"));
    assert!(response.contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo="));
}

#[tokio::test]
async fn handshake_missing_key_returns_error() {
    let (_client, mut server) = duplex(1024);
    let headers = Headers::new();
    let err = WebSocket::handshake(&mut server, &headers).await.unwrap_err();
    assert!(err.to_string().contains("missing Sec-WebSocket-Key"));
}

#[tokio::test]
async fn run_errors_when_reader_missing() {
    let a = addr(9300);
    let global = Arc::new(GlobalContext::new(a, None));
    let mut ctx = Context::new(None, None, global, a);
    let err = WebSocket::run(&WebSocket::new(), &mut ctx).await.unwrap_err();
    assert!(err.to_string().contains("Reader missing"));
}

#[tokio::test]
async fn run_errors_when_writer_missing() {
    let (r, _w) = duplex(64);
    let reader: Option<BoxReader> = Some(Box::new(tokio::io::BufReader::new(r)));
    let a = addr(9301);
    let global = Arc::new(GlobalContext::new(a, None));
    let mut ctx = Context::new(reader, None, global, a);
    let err = WebSocket::run(&WebSocket::new(), &mut ctx).await.unwrap_err();
    assert!(err.to_string().contains("Writer missing"));
}

#[tokio::test]
async fn to_middleware_passes_through_non_ws_request() {
    let a = addr(9302);
    let global = Arc::new(GlobalContext::new(a, None));
    let mut ctx = Context::new(None, None, global, a);
    ctx.local.set_value(HttpMetadata::new());
    let mw = WebSocket::to_middleware(WebSocket::new());
    assert!(mw(&mut ctx).await);
}

#[tokio::test]
async fn to_middleware_passes_through_without_metadata() {
    let a = addr(9303);
    let global = Arc::new(GlobalContext::new(a, None));
    let mut ctx = Context::new(None, None, global, a);
    let mw = WebSocket::to_middleware(WebSocket::new());
    assert!(mw(&mut ctx).await);
}

#[tokio::test]
async fn to_middleware_returns_false_when_no_writer() {
    let a = addr(9304);
    let global = Arc::new(GlobalContext::new(a, None));
    let mut ctx = Context::new(None, None, global, a);
    let mut meta = HttpMetadata::new();
    meta.headers.insert(HeaderKey::Upgrade, "websocket".to_string());
    meta.headers.insert(HeaderKey::Connection, "Upgrade".to_string());
    ctx.local.set_value(meta);
    let mw = WebSocket::to_middleware(WebSocket::new());
    assert!(!mw(&mut ctx).await);
}

#[tokio::test]
async fn ws_sender_list_broadcast_and_removes_dead_senders() {
    let list = WsSenderList::new();
    assert_eq!(list.len().await, 0);

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<WSFrame>();
    list.senders.lock().await.push(tx);
    assert_eq!(list.len().await, 1);

    list.broadcast("hi").await;
    match rx.recv().await {
        Some(WSFrame::Text(s)) => assert_eq!(s, "hi"),
        other => panic!("expected Text frame, got {other:?}"),
    }

    drop(rx);
    list.broadcast("bye").await;
    assert_eq!(list.len().await, 0);
}

#[test]
fn set_handler_is_noop_and_keeps_fields() {
    let ws = WebSocket::new().set_handler(|_ws, _ctx, _frame| Box::pin(async move { true }));
    assert!(ws.on_text.is_none());
    assert!(ws.on_binary.is_none());
}
