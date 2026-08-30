use aex::http2::H2Codec;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};

fn make_http_router() -> aex::http::router::Router {
    use aex::connection::context::Context;
    let mut router = aex::http::router::Router::default();
    router.get("/", |_ctx: &mut Context| "hello-h2");
    router
}

#[tokio::test]
async fn h2_is_h2_connection_true_for_preface() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mut client = TcpStream::connect(addr).await.unwrap();
    client
        .write_all(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n")
        .await
        .unwrap();
    client.flush().await.unwrap();
    let (mut server, _) = listener.accept().await.unwrap();
    assert!(H2Codec::is_h2_connection(&mut server).await);
}

#[tokio::test]
async fn h2_is_h2_connection_false_for_http11() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mut client = TcpStream::connect(addr).await.unwrap();
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n")
        .await
        .unwrap();
    client.flush().await.unwrap();
    let (mut server, _) = listener.accept().await.unwrap();
    assert!(!H2Codec::is_h2_connection(&mut server).await);
}

#[tokio::test]
async fn h2_is_h2_connection_false_for_short_data() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mut client = TcpStream::connect(addr).await.unwrap();
    client.write_all(b"PRI * ").await.unwrap();
    client.flush().await.unwrap();
    let (mut server, _) = listener.accept().await.unwrap();
    assert!(!H2Codec::is_h2_connection(&mut server).await);
}

#[tokio::test]
async fn h2_codec_handle_serves_request() {
    use h2::client;
    use std::sync::Arc;

    let global = Arc::new(aex::connection::global::GlobalContext::new(
        "127.0.0.1:0".parse().unwrap(),
        None,
    ));
    let codec = Arc::new(H2Codec::new(Arc::new(make_http_router()), global));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server_task = tokio::spawn(async move {
        let (socket, peer) = listener.accept().await.unwrap();
        let token = tokio_util::sync::CancellationToken::new();
        let _ = codec.handle(socket, peer, token).await;
    });

    let client_stream = TcpStream::connect(addr).await.unwrap();
    let (mut send_request, mut conn) = client::handshake(client_stream).await.unwrap();
    tokio::spawn(async move {
        let _ = conn.await;
    });

    let request = http::Request::builder()
        .method("GET")
        .uri("http://localhost/")
        .body(())
        .unwrap();
    let (response, _) = send_request.send_request(request, false).unwrap();
    let resp = tokio::time::timeout(std::time::Duration::from_secs(5), response)
        .await
        .expect("response timeout")
        .unwrap();
    assert_eq!(resp.status(), http::StatusCode::OK);

    let mut body = resp.into_body();
    let mut body_bytes = Vec::new();
    // h2 RecvStream::data() 返回 Future（每次取一帧）
    while let Some(Ok(chunk)) = body.data().await {
        body_bytes.extend_from_slice(&chunk);
    }
    assert_eq!(String::from_utf8_lossy(&body_bytes), "hello-h2");
    server_task.abort();
}

#[tokio::test]
async fn h2_codec_handle_handshake_failure() {
    use std::sync::Arc;

    let global = Arc::new(aex::connection::global::GlobalContext::new(
        "127.0.0.1:0".parse().unwrap(),
        None,
    ));
    let codec = Arc::new(H2Codec::new(Arc::new(make_http_router()), global));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server_task = tokio::spawn(async move {
        let (socket, peer) = listener.accept().await.unwrap();
        let token = tokio_util::sync::CancellationToken::new();
        codec.handle(socket, peer, token).await
    });

    // 客户端连上但不发送 h2 preface → 握手失败
    let mut client = TcpStream::connect(addr).await.unwrap();
    client.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n").await.unwrap();
    client.flush().await.unwrap();

    let result = tokio::time::timeout(std::time::Duration::from_secs(3), server_task)
        .await
        .expect("server task timeout")
        .unwrap();
    assert!(result.is_err());
}
