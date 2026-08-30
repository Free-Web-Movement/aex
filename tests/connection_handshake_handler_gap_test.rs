use std::net::SocketAddr;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;

use aex::connection::commands::{AckCommand, HelloCommand, RejectCommand, WelcomeCommand};
use aex::connection::context::Context;
use aex::connection::global::GlobalContext;
use aex::connection::handshake_handler::HandshakeHandler;
use aex::connection::node::Node;

fn create_test_node() -> Node {
    Node::from_addr("127.0.0.1:8080".parse().unwrap(), Some(1), Some(vec![1; 32]))
}

async fn preload_reader(request: &[u8]) -> (Arc<Mutex<Context>>, tokio::io::DuplexStream) {
    let peer: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    let (mut req_writer, req_reader) = tokio::io::duplex(1024);
    req_writer.write_all(request).await.unwrap();
    drop(req_writer);

    let (resp_reader, resp_writer) = tokio::io::duplex(1024);
    let global = Arc::new(GlobalContext::new(peer, None));
    let ctx = Arc::new(Mutex::new(Context::new(
        Some(Box::new(BufReader::new(req_reader))),
        Some(Box::new(resp_writer)),
        global,
        peer,
    )));
    (ctx, resp_reader)
}

fn length_prefixed(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
    out
}

#[tokio::test]
async fn test_handle_server_side_no_reader() {
    let peer: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    let global = Arc::new(GlobalContext::new(peer, None));
    let ctx = Arc::new(Mutex::new(Context::new(None, None, global, peer)));
    let handler = HandshakeHandler::new(create_test_node());

    let result = handler.handle_server_side(ctx, peer).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_handle_server_side_too_large() {
    let peer: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    let (mut req_writer, req_reader) = tokio::io::duplex(1024);
    req_writer.write_all(&5000u32.to_le_bytes()).await.unwrap();
    drop(req_writer);

    let (resp_reader, resp_writer) = tokio::io::duplex(1024);
    let global = Arc::new(GlobalContext::new(peer, None));
    let ctx = Arc::new(Mutex::new(Context::new(
        Some(Box::new(BufReader::new(req_reader))),
        Some(Box::new(resp_writer)),
        global,
        peer,
    )));
    let handler = HandshakeHandler::new(create_test_node());

    let result = handler.handle_server_side(ctx, peer).await;
    assert!(result.is_err());
    drop(resp_reader);
}

#[tokio::test]
async fn test_handle_server_side_unknown_command() {
    let peer: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    let ack = AckCommand::accepted(None);
    let req = length_prefixed(&ack.encode());
    let (ctx, _resp_reader) = preload_reader(&req).await;
    let handler = HandshakeHandler::new(create_test_node());

    let result = handler.handle_server_side(ctx, peer).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_handle_server_side_reject_invokes_callback() {
    let peer: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    let called = Arc::new(std::sync::Mutex::new(String::new()));
    let c = called.clone();
    let handler = HandshakeHandler::new(create_test_node()).on_rejected(move |reason, _addr| {
        *c.lock().unwrap() = reason;
    });

    let reject = RejectCommand::new("busy");
    let req = length_prefixed(&reject.encode());
    let (ctx, _resp_reader) = preload_reader(&req).await;

    let result = handler.handle_server_side(ctx, peer).await;
    assert!(result.is_err());
    assert_eq!(*called.lock().unwrap(), "busy");
}

#[tokio::test]
async fn test_handle_server_side_hello_success() {
    use tokio::io::AsyncReadExt;
    let peer: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    let called = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let c = called.clone();
    let handler = HandshakeHandler::new(create_test_node()).on_established(move |_n, _a| {
        c.store(true, std::sync::atomic::Ordering::SeqCst);
    });

    let hello = HelloCommand::new(create_test_node(), None, false);
    let req = length_prefixed(&hello.encode());
    let (ctx, mut resp_reader) = preload_reader(&req).await;

    let result = handler.handle_server_side(ctx, peer).await;
    assert!(result.is_ok());
    assert!(called.load(std::sync::atomic::Ordering::SeqCst));

    // 应收到 welcome 帧（4 字节长度 + data）
    let mut len_buf = [0u8; 4];
    resp_reader.read_exact(&mut len_buf).await.unwrap();
    let len = u32::from_le_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    resp_reader.read_exact(&mut buf).await.unwrap();
    let welcome = WelcomeCommand::decode(&buf).unwrap();
    assert!(welcome.accepted);
}

#[tokio::test]
async fn test_handle_server_side_version_mismatch() {
    use tokio::io::AsyncReadExt;
    let peer: SocketAddr = "127.0.0.1:8080".parse().unwrap();

    // 构造一个 invalid hello（版本不匹配）
    let node = Node::from_addr(
        "127.0.0.1:8080".parse().unwrap(),
        Some(1),
        Some(vec![1; 32]),
    );
    let hello = HelloCommand {
        version: 99, // 版本 99 → is_valid false
        node,
        ephemeral_public: None,
        request_encryption: false,
    };
    let req = length_prefixed(&hello.encode());
    let (ctx, mut resp_reader) = preload_reader(&req).await;

    let handler = HandshakeHandler::new(create_test_node());
    let result = handler.handle_server_side(ctx, peer).await;
    assert!(result.is_err());
    let err = format!("{}", result.unwrap_err());
    assert!(err.contains("version mismatch"), "err={err}");

    // 应收到 reject 帧
    let mut len_buf = [0u8; 4];
    resp_reader.read_exact(&mut len_buf).await.unwrap();
    let len = u32::from_le_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    resp_reader.read_exact(&mut buf).await.unwrap();
    let reject = RejectCommand::decode(&buf).unwrap();
    assert_eq!(reject.reason, "version mismatch");
}

#[tokio::test]
async fn test_handshake_as_client_success() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let peer = listener.local_addr().unwrap();
    let node = create_test_node();

    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut len_buf = [0u8; 4];
        socket.read_exact(&mut len_buf).await.unwrap();
        let len = u32::from_le_bytes(len_buf) as usize;
        let mut data = vec![0u8; len];
        socket.read_exact(&mut data).await.unwrap();
        let hello = HelloCommand::decode(&data).unwrap();
        let welcome = WelcomeCommand::new(hello.node.clone(), true, None);
        let enc = welcome.encode();
        socket
            .write_all(&(enc.len() as u32).to_le_bytes())
            .await
            .unwrap();
        socket.write_all(&enc).await.unwrap();
        socket.flush().await.unwrap();
        hello.node
    });

    let established = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let e = established.clone();
    let handler = HandshakeHandler::new(node.clone()).on_established(move |_n, _a| {
        e.store(true, std::sync::atomic::Ordering::SeqCst);
    });

    let got = handler.handshake_as_client(peer, false).await.unwrap();
    assert_eq!(got.id, node.id);
    assert!(established.load(std::sync::atomic::Ordering::SeqCst));
    server.await.unwrap();
}

#[tokio::test]
async fn test_handshake_as_client_rejected() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let peer = listener.local_addr().unwrap();

    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut len_buf = [0u8; 4];
        socket.read_exact(&mut len_buf).await.unwrap();
        let len = u32::from_le_bytes(len_buf) as usize;
        let mut data = vec![0u8; len];
        socket.read_exact(&mut data).await.unwrap();
        let reject = RejectCommand::new("busy");
        let enc = reject.encode();
        socket
            .write_all(&(enc.len() as u32).to_le_bytes())
            .await
            .unwrap();
        socket.write_all(&enc).await.unwrap();
        socket.flush().await.unwrap();
    });

    let handler = HandshakeHandler::new(create_test_node());
    let result = handler.handshake_as_client(peer, false).await;
    assert!(result.is_err());
    let err = format!("{}", result.unwrap_err());
    assert!(err.contains("rejected"), "err={err}");
    server.await.unwrap();
}

#[tokio::test]
async fn test_handshake_as_client_unexpected_message() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let peer = listener.local_addr().unwrap();

    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut len_buf = [0u8; 4];
        socket.read_exact(&mut len_buf).await.unwrap();
        let len = u32::from_le_bytes(len_buf) as usize;
        let mut data = vec![0u8; len];
        socket.read_exact(&mut data).await.unwrap();
        let ack = AckCommand::accepted(None);
        let enc = ack.encode();
        socket
            .write_all(&(enc.len() as u32).to_le_bytes())
            .await
            .unwrap();
        socket.write_all(&enc).await.unwrap();
        socket.flush().await.unwrap();
    });

    let handler = HandshakeHandler::new(create_test_node());
    let result = handler.handshake_as_client(peer, false).await;
    assert!(result.is_err());
    server.await.unwrap();
}
