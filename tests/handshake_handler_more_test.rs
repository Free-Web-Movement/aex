use std::net::SocketAddr;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;

use aex::connection::commands::{CommandId, HelloCommand, RejectCommand, WelcomeCommand};
use aex::connection::context::Context;
use aex::connection::global::GlobalContext;
use aex::connection::handshake_handler::HandshakeHandler;
use aex::connection::node::Node;
use aex::crypto::session_key_manager::PairedSessionKey;

fn create_test_node() -> Node {
    Node::from_addr(
        "127.0.0.1:8080".parse().unwrap(),
        Some(1),
        Some(vec![1; 32]),
    )
}

fn length_prefixed(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
    out
}

fn malformed_command(id: CommandId) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&id.as_u32().to_le_bytes());
    data.extend_from_slice(&[0xFF, 0xFE, 0xFD]);
    data
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

async fn read_frame(stream: &mut tokio::io::DuplexStream) -> Vec<u8> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await.unwrap();
    let len = u32::from_le_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await.unwrap();
    buf
}

#[tokio::test]
async fn handle_server_side_hello_with_encryption_and_keys_returns_ephemeral() {
    let peer: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    let keys = Arc::new(Mutex::new(PairedSessionKey::new(32)));
    let handler = HandshakeHandler::new(create_test_node()).with_session_keys(keys);

    let hello = HelloCommand::new(create_test_node(), Some(vec![7u8; 32]), true);
    let req = length_prefixed(&hello.encode());
    let (ctx, mut resp_reader) = preload_reader(&req).await;

    let result = handler.handle_server_side(ctx, peer).await;
    assert!(result.is_ok());

    let frame = read_frame(&mut resp_reader).await;
    let welcome = WelcomeCommand::decode(&frame).unwrap();
    assert!(welcome.accepted);
    assert!(welcome.ephemeral_public.is_some());
}

#[tokio::test]
async fn handle_server_side_hello_without_writer_errors() {
    let peer: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    let (mut req_writer, req_reader) = tokio::io::duplex(1024);
    let hello = HelloCommand::new(create_test_node(), None, false);
    req_writer
        .write_all(&length_prefixed(&hello.encode()))
        .await
        .unwrap();
    drop(req_writer);

    let global = Arc::new(GlobalContext::new(peer, None));
    let ctx = Arc::new(Mutex::new(Context::new(
        Some(Box::new(BufReader::new(req_reader))),
        None,
        global,
        peer,
    )));

    let handler = HandshakeHandler::new(create_test_node());
    let result = handler.handle_server_side(ctx, peer).await;
    assert!(result.is_err());
    assert!(format!("{}", result.unwrap_err()).contains("no writer"));
}

#[tokio::test]
async fn handle_server_side_version_mismatch_without_writer_errors() {
    let peer: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    let (mut req_writer, req_reader) = tokio::io::duplex(1024);
    let hello = HelloCommand {
        version: 99,
        node: create_test_node(),
        ephemeral_public: None,
        request_encryption: false,
    };
    req_writer
        .write_all(&length_prefixed(&hello.encode()))
        .await
        .unwrap();
    drop(req_writer);

    let global = Arc::new(GlobalContext::new(peer, None));
    let ctx = Arc::new(Mutex::new(Context::new(
        Some(Box::new(BufReader::new(req_reader))),
        None,
        global,
        peer,
    )));

    let handler = HandshakeHandler::new(create_test_node());
    let result = handler.handle_server_side(ctx, peer).await;
    assert!(result.is_err());
    assert!(format!("{}", result.unwrap_err()).contains("no writer"));
}

#[tokio::test]
async fn handle_server_side_hello_bad_json_errors() {
    let peer: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    let req = length_prefixed(&malformed_command(CommandId::Hello));
    let (ctx, _resp_reader) = preload_reader(&req).await;

    let handler = HandshakeHandler::new(create_test_node());
    let result = handler.handle_server_side(ctx, peer).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn handle_server_side_reject_bad_json_errors() {
    let peer: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    let req = length_prefixed(&malformed_command(CommandId::Reject));
    let (ctx, _resp_reader) = preload_reader(&req).await;

    let handler = HandshakeHandler::new(create_test_node());
    let result = handler.handle_server_side(ctx, peer).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn handle_server_side_hello_request_encryption_without_keys_no_ephemeral() {
    let peer: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    let handler = HandshakeHandler::new(create_test_node());

    let hello = HelloCommand::new(create_test_node(), None, true);
    let req = length_prefixed(&hello.encode());
    let (ctx, mut resp_reader) = preload_reader(&req).await;

    let result = handler.handle_server_side(ctx, peer).await;
    assert!(result.is_ok());

    let frame = read_frame(&mut resp_reader).await;
    let welcome = WelcomeCommand::decode(&frame).unwrap();
    assert!(welcome.ephemeral_public.is_none());
}

async fn spawn_welcome_server(welcome_bytes: Vec<u8>) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let peer = listener.local_addr().unwrap();

    let handle = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut len_buf = [0u8; 4];
        socket.read_exact(&mut len_buf).await.unwrap();
        let len = u32::from_le_bytes(len_buf) as usize;
        let mut data = vec![0u8; len];
        socket.read_exact(&mut data).await.unwrap();

        socket
            .write_all(&(welcome_bytes.len() as u32).to_le_bytes())
            .await
            .unwrap();
        socket.write_all(&welcome_bytes).await.unwrap();
        socket.flush().await.unwrap();
    });

    (peer, handle)
}

#[tokio::test]
async fn handshake_as_client_welcome_not_accepted_errors() {
    let node = create_test_node();
    let welcome = WelcomeCommand::new(node, false, None);
    let (peer, server) = spawn_welcome_server(welcome.encode()).await;

    let handler = HandshakeHandler::new(create_test_node());
    let result = handler.handshake_as_client(peer, false).await;
    assert!(result.is_err());
    assert!(format!("{}", result.unwrap_err()).contains("connection rejected"));
    server.await.unwrap();
}

#[tokio::test]
async fn handshake_as_client_welcome_bad_json_errors() {
    let (peer, server) = spawn_welcome_server(malformed_command(CommandId::Welcome)).await;

    let handler = HandshakeHandler::new(create_test_node());
    let result = handler.handshake_as_client(peer, false).await;
    assert!(result.is_err());
    server.await.unwrap();
}

#[tokio::test]
async fn handshake_as_client_reject_bad_json_errors() {
    let (peer, server) = spawn_welcome_server(malformed_command(CommandId::Reject)).await;

    let handler = HandshakeHandler::new(create_test_node());
    let result = handler.handshake_as_client(peer, false).await;
    assert!(result.is_err());
    server.await.unwrap();
}

#[tokio::test]
async fn handshake_as_client_reject_carries_reason() {
    let (peer, server) = spawn_welcome_server(RejectCommand::new("busy").encode()).await;

    let handler = HandshakeHandler::new(create_test_node());
    let result = handler.handshake_as_client(peer, false).await;
    assert!(result.is_err());
    assert!(format!("{}", result.unwrap_err()).contains("busy"));
    server.await.unwrap();
}
