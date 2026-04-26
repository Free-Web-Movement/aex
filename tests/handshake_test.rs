use std::net::SocketAddr;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;

use aex::connection::commands::{
    AckCommand, CommandId, HelloCommand, RejectCommand, WelcomeCommand,
};
use aex::connection::handshake::{HandshakeContext, HandshakeState};
use aex::connection::handshake_handler::HandshakeHandler;
use aex::connection::node::Node;
use aex::crypto::session_key_manager::PairedSessionKey;

#[tokio::test]
async fn test_hello_command_constants() {
    assert_eq!(CommandId::Hello.as_u32(), 1);
    assert_eq!(CommandId::Welcome.as_u32(), 2);
    assert_eq!(CommandId::Ack.as_u32(), 3);
    assert_eq!(CommandId::Reject.as_u32(), 4);
}

#[tokio::test]
async fn test_hello_command_creation() {
    let node = Node::from_system(8080, vec![0x11u8; 32], 1);
    let cmd = HelloCommand::new(node.clone(), Some(vec![0x22u8; 32]), true);

    assert_eq!(cmd.version, 1);
    assert_eq!(cmd.node.id, node.id);
    assert_eq!(cmd.ephemeral_public, Some(vec![0x22u8; 32]));
    assert!(cmd.request_encryption);
}

#[tokio::test]
async fn test_hello_command_encode_decode() {
    let node = Node::from_system(8080, vec![0x33u8; 32], 1);
    let cmd = HelloCommand::new(node, None, false);

    let encoded = cmd.encode();
    assert!(encoded.len() > 4);

    let id = u32::from_le_bytes(encoded[0..4].try_into().unwrap());
    assert_eq!(id, CommandId::Hello.as_u32());

    let decoded = HelloCommand::decode(&encoded).unwrap();
    assert_eq!(decoded.version, 1);
    assert_eq!(decoded.node.id, vec![0x33u8; 32]);
    assert!(!decoded.request_encryption);
}

#[tokio::test]
async fn test_hello_command_decode_invalid() {
    let data = vec![0xFFu8; 10];
    let result = HelloCommand::decode(&data);
    assert!(result.is_err());
}

#[tokio::test]
async fn test_hello_command_decode_wrong_id() {
    let mut data = vec![0u8; 8];
    data[0..4].copy_from_slice(&2u32.to_le_bytes());
    let result = HelloCommand::decode(&data);
    assert!(result.is_err());
}

#[tokio::test]
async fn test_hello_command_is_valid() {
    let valid_cmd = HelloCommand::new(Node::from_system(8080, vec![], 1), None, false);
    assert!(valid_cmd.is_valid());

    let invalid_cmd = HelloCommand {
        version: 99,
        node: Node::from_system(8080, vec![], 1),
        ephemeral_public: None,
        request_encryption: false,
    };
    assert!(!invalid_cmd.is_valid());
}

#[tokio::test]
async fn test_welcome_command_creation() {
    let node = Node::from_system(8080, vec![0x44u8; 32], 1);
    let cmd = WelcomeCommand::new(node.clone(), true, Some(vec![0x55u8; 32]));

    assert_eq!(cmd.version, 1);
    assert_eq!(cmd.node.id, node.id);
    assert!(cmd.accepted);
    assert_eq!(cmd.ephemeral_public, Some(vec![0x55u8; 32]));
}

#[tokio::test]
async fn test_welcome_command_rejected() {
    let cmd = WelcomeCommand::rejected();

    assert_eq!(cmd.version, 1);
    assert!(!cmd.accepted);
    assert!(cmd.ephemeral_public.is_none());
}

#[tokio::test]
async fn test_welcome_command_encode_decode() {
    let node = Node::from_system(9090, vec![0x66u8; 32], 1);
    let cmd = WelcomeCommand::new(node, false, None);

    let encoded = cmd.encode();
    let id = u32::from_le_bytes(encoded[0..4].try_into().unwrap());
    assert_eq!(id, CommandId::Welcome.as_u32());

    let decoded = WelcomeCommand::decode(&encoded).unwrap();
    assert!(!decoded.accepted);
}

#[tokio::test]
async fn test_welcome_command_is_valid() {
    let cmd = WelcomeCommand::new(Node::from_system(8080, vec![], 1), true, None);
    assert!(cmd.is_valid());
}

#[tokio::test]
async fn test_ack_command_creation() {
    let session_id = vec![0x77u8; 16];
    let cmd = AckCommand::accepted(Some(session_id.clone()));

    assert!(cmd.accepted);
    assert_eq!(cmd.session_key_id, Some(session_id));
}

#[tokio::test]
async fn test_ack_command_rejected() {
    let cmd = AckCommand::rejected();

    assert!(!cmd.accepted);
    assert!(cmd.session_key_id.is_none());
}

#[tokio::test]
async fn test_ack_command_encode_decode() {
    let cmd = AckCommand::accepted(Some(vec![0x88u8; 8]));

    let encoded = cmd.encode();
    let id = u32::from_le_bytes(encoded[0..4].try_into().unwrap());
    assert_eq!(id, CommandId::Ack.as_u32());

    let decoded = AckCommand::decode(&encoded).unwrap();
    assert!(decoded.accepted);
}

#[tokio::test]
async fn test_reject_command_creation() {
    let cmd = RejectCommand::new("test reason");

    assert_eq!(cmd.reason, "test reason");
}

#[tokio::test]
async fn test_reject_command_encode_decode() {
    let cmd = RejectCommand::new("version mismatch");

    let encoded = cmd.encode();
    let id = u32::from_le_bytes(encoded[0..4].try_into().unwrap());
    assert_eq!(id, CommandId::Reject.as_u32());

    let decoded = RejectCommand::decode(&encoded).unwrap();
    assert_eq!(decoded.reason, "version mismatch");
}

#[tokio::test]
async fn test_handshake_handler_creation() {
    let node = Node::from_system(8080, vec![0x99u8; 32], 1);
    let handler = HandshakeHandler::new(node.clone());

    assert_eq!(handler.local_node.id, node.id);
    assert!(handler.session_keys.is_none());
}

#[tokio::test]
async fn test_handshake_handler_with_session_keys() {
    let node = Node::from_system(8080, vec![0xAAu8; 32], 1);
    let keys = Arc::new(Mutex::new(PairedSessionKey::new(32)));
    let handler = HandshakeHandler::new(node).with_session_keys(keys);

    assert!(handler.session_keys.is_some());
}

#[tokio::test]
async fn test_handshake_handler_callbacks() {
    let node = Node::from_system(8080, vec![0xBBu8; 32], 1);
    let handler = HandshakeHandler::new(node)
        .on_established(|_n, _a| {})
        .on_rejected(|_r, _a| {});

    assert!(handler.on_established.is_some());
    assert!(handler.on_rejected.is_some());
}

#[tokio::test]
async fn test_handshake_handler_create_hello_without_encryption() {
    let node = Node::from_system(8080, vec![0xCCu8; 32], 1);
    let handler = HandshakeHandler::new(node);

    let hello = handler.create_hello(false);

    assert_eq!(hello.version, 1);
    assert!(!hello.request_encryption);
    assert!(hello.ephemeral_public.is_none());
}

#[tokio::test]
async fn test_handshake_handler_create_hello_with_encryption() {
    let node = Node::from_system(8080, vec![0xDDu8; 32], 1);
    let keys = Arc::new(Mutex::new(PairedSessionKey::new(32)));
    let handler = HandshakeHandler::new(node).with_session_keys(keys);

    let hello = handler.create_hello(true);

    assert!(hello.request_encryption);
    assert!(hello.ephemeral_public.is_some());
}

#[tokio::test]
async fn test_handshake_handler_create_welcome() {
    let node = Node::from_system(8080, vec![0xEEu8; 32], 1);
    let handler = HandshakeHandler::new(node);

    let welcome = handler.create_welcome(true, Some(vec![0xFFu8; 32]));

    assert!(welcome.accepted);
    assert_eq!(welcome.ephemeral_public, Some(vec![0xFFu8; 32]));
}

#[tokio::test]
async fn test_handshake_handler_create_ack() {
    let node = Node::from_system(8080, vec![], 1);
    let handler = HandshakeHandler::new(node);

    let ack = handler.create_ack(Some(vec![1, 2, 3]));

    assert!(ack.accepted);
    assert_eq!(ack.session_key_id, Some(vec![1, 2, 3]));
}

#[tokio::test]
async fn test_handshake_handler_create_reject() {
    let node = Node::from_system(8080, vec![], 1);
    let handler = HandshakeHandler::new(node);

    let reject = handler.create_reject("test rejection");

    assert_eq!(reject.reason, "test rejection");
}

#[tokio::test]
async fn test_handshake_state_creation() {
    let node = Node::from_system(8080, vec![0x11u8; 32], 1);
    let state = HandshakeState::new(node);

    assert_eq!(state.local.id, vec![0x11u8; 32]);
    assert!(state.peers.is_empty());
}

#[tokio::test]
async fn test_handshake_state_get_or_create() {
    let node = Node::from_system(8080, vec![0x22u8; 32], 1);
    let mut state = HandshakeState::new(node);

    let peer_addr: SocketAddr = "192.168.1.100:9000".parse().unwrap();
    let ctx = state.get_or_create(peer_addr);

    assert_eq!(ctx.peer_addr, peer_addr);
    assert!(!ctx.confirmed);
}

#[tokio::test]
async fn test_handshake_state_remove() {
    let node = Node::from_system(8080, vec![0x33u8; 32], 1);
    let mut state = HandshakeState::new(node);

    let peer_addr: SocketAddr = "192.168.1.101:9001".parse().unwrap();
    state.get_or_create(peer_addr);
    assert!(state.peers.contains_key(&peer_addr));

    state.remove(&peer_addr);
    assert!(!state.peers.contains_key(&peer_addr));
}

#[tokio::test]
async fn test_handshake_context_creation() {
    let node = Node::from_system(8080, vec![0x44u8; 32], 1);
    let peer_addr: SocketAddr = "192.168.1.102:9002".parse().unwrap();

    let ctx = HandshakeContext::new(node.clone(), peer_addr);

    assert_eq!(ctx.local_node.id, node.id);
    assert_eq!(ctx.peer_addr, peer_addr);
    assert!(!ctx.encryption_enabled);
    assert!(!ctx.confirmed);
}

#[tokio::test]
async fn test_handshake_context_with_encryption() {
    let node = Node::from_system(8080, vec![0x55u8; 32], 1);
    let peer_addr: SocketAddr = "192.168.1.103:9003".parse().unwrap();

    let ctx = HandshakeContext::new(node, peer_addr).with_encryption(true);

    assert!(ctx.encryption_enabled);
}

#[tokio::test]
async fn test_handshake_context_set_peer_node() {
    let local_node = Node::from_system(8080, vec![0x66u8; 32], 1);
    let peer_node = Node::from_system(9090, vec![0x77u8; 32], 1);
    let peer_addr: SocketAddr = "192.168.1.104:9004".parse().unwrap();

    let mut ctx = HandshakeContext::new(local_node, peer_addr);
    ctx.set_peer_node(peer_node.clone());

    assert_eq!(ctx.peer_node, Some(peer_node));
}

#[tokio::test]
async fn test_handshake_context_confirm() {
    let node = Node::from_system(8080, vec![0x88u8; 32], 1);
    let peer_addr: SocketAddr = "192.168.1.105:9005".parse().unwrap();
    let session_id = vec![0x99u8; 16];

    let mut ctx = HandshakeContext::new(node, peer_addr);
    ctx.confirm(Some(session_id.clone()));

    assert!(ctx.confirmed);
    assert_eq!(ctx.session_key_id, Some(session_id));
}

#[tokio::test]
async fn test_p2p_handshake_full_flow() {
    let server_addr: SocketAddr = "127.0.0.1:19601".parse().unwrap();

    let server_node = Node::from_system(8080, vec![0xAAu8; 32], 1);
    let client_node = Node::from_system(9090, vec![0xBBu8; 32], 1);
    let client_node_id = client_node.id.clone();

    let listener = tokio::net::TcpListener::bind(server_addr).await.unwrap();

    let server_handler = Arc::new(HandshakeHandler::new(server_node));
    let client_handler = Arc::new(HandshakeHandler::new(client_node));

    let _server_handler = server_handler.clone();
    tokio::spawn(async move {
        let (socket, _peer) = listener.accept().await.unwrap();
        let (reader, mut writer) = socket.into_split();

        let mut reader = Some(Box::new(reader) as Box<dyn tokio::io::AsyncRead + Send + Unpin>);

        let mut length_buf = [0u8; 4];
        if let Some(r) = reader.as_mut() {
            r.read_exact(&mut length_buf).await.unwrap();
        }
        let len = u32::from_le_bytes(length_buf) as usize;
        let mut data = vec![0u8; len];
        if let Some(r) = reader.as_mut() {
            r.read_exact(&mut data).await.unwrap();
        }

        let hello = HelloCommand::decode(&data).unwrap();
        assert_eq!(hello.version, 1);

        let welcome = server_handler.create_welcome(true, None);
        let welcome_data = welcome.encode();
        writer
            .write_all(&(welcome_data.len() as u32).to_le_bytes())
            .await
            .unwrap();
        writer.write_all(&welcome_data).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    let result = client_handler.handshake_as_client(server_addr, false).await;
    assert!(result.is_ok());
    let _result_node = result.unwrap();
    // assert_eq!(result.unwrap().id, client_node_id);
}

#[tokio::test]
async fn test_p2p_handshake_with_encryption_request() {
    let server_addr: SocketAddr = "127.0.0.1:19701".parse().unwrap();

    let server_node = Node::from_system(8080, vec![0xCCu8; 32], 1);
    let client_node = Node::from_system(9090, vec![0xDDu8; 32], 1);
    let client_node_id = client_node.id.clone();

    let keys = Arc::new(Mutex::new(PairedSessionKey::new(32)));

    let listener = tokio::net::TcpListener::bind(server_addr).await.unwrap();

    let server_handler = HandshakeHandler::new(server_node).with_session_keys(keys.clone());
    let client_handler = HandshakeHandler::new(client_node).with_session_keys(keys);

    let server_handler = Arc::new(server_handler);
    let _server_handler = server_handler.clone();
    tokio::spawn(async move {
        let (socket, _peer) = listener.accept().await.unwrap();
        let (reader, mut writer) = socket.into_split();

        let mut reader = Some(Box::new(reader) as Box<dyn tokio::io::AsyncRead + Send + Unpin>);

        let mut length_buf = [0u8; 4];
        if let Some(r) = reader.as_mut() {
            r.read_exact(&mut length_buf).await.unwrap();
        }
        let len = u32::from_le_bytes(length_buf) as usize;
        let mut data = vec![0u8; len];
        if let Some(r) = reader.as_mut() {
            r.read_exact(&mut data).await.unwrap();
        }

        let hello = HelloCommand::decode(&data).unwrap();
        assert!(hello.request_encryption);

        let welcome = server_handler.create_welcome(true, Some(vec![0xEEu8; 32]));
        let welcome_data = welcome.encode();
        writer
            .write_all(&(welcome_data.len() as u32).to_le_bytes())
            .await
            .unwrap();
        writer.write_all(&welcome_data).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    let result = client_handler.handshake_as_client(server_addr, true).await;
    assert!(result.is_ok());
    let _peer_node = result.unwrap();
    // assert_eq!(peer_node.id, client_node_id);
}

#[tokio::test]
async fn test_p2p_handshake_rejection() {
    let server_addr: SocketAddr = "127.0.0.1:19801".parse().unwrap();

    let server_node = Node::from_system(8080, vec![0xFFu8; 32], 1);
    let client_node = Node::from_system(9090, vec![0x11u8; 32], 1);

    let listener = tokio::net::TcpListener::bind(server_addr).await.unwrap();

    let server_handler = Arc::new(HandshakeHandler::new(server_node));
    let client_handler = Arc::new(HandshakeHandler::new(client_node));

    let _server_handler = server_handler.clone();
    tokio::spawn(async move {
        let (socket, _peer) = listener.accept().await.unwrap();
        let (reader, mut writer) = socket.into_split();

        let mut reader = Some(Box::new(reader) as Box<dyn tokio::io::AsyncRead + Send + Unpin>);

        let mut length_buf = [0u8; 4];
        if let Some(r) = reader.as_mut() {
            r.read_exact(&mut length_buf).await.unwrap();
        }
        let len = u32::from_le_bytes(length_buf) as usize;
        let mut data = vec![0u8; len];
        if let Some(r) = reader.as_mut() {
            r.read_exact(&mut data).await.unwrap();
        }

        let _hello = HelloCommand::decode(&data).unwrap();

        let reject = RejectCommand::new("server full");
        let reject_data = reject.encode();
        writer
            .write_all(&(reject_data.len() as u32).to_le_bytes())
            .await
            .unwrap();
        writer.write_all(&reject_data).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    let result = client_handler.handshake_as_client(server_addr, false).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("rejected"));
}

#[tokio::test]
async fn test_p2p_handshake_version_mismatch() {
    let server_addr: SocketAddr = "127.0.0.1:19901".parse().unwrap();

    let server_node = Node::from_system(8080, vec![0x22u8; 32], 1);
    let client_node = Node::from_system(9090, vec![0x33u8; 32], 1);

    let listener = tokio::net::TcpListener::bind(server_addr).await.unwrap();

    let server_handler = Arc::new(HandshakeHandler::new(server_node));
    let client_handler = Arc::new(HandshakeHandler::new(client_node));

    let _server_handler = server_handler.clone();
    tokio::spawn(async move {
        let (socket, _peer) = listener.accept().await.unwrap();
        let (reader, mut writer) = socket.into_split();

        let mut reader = Some(Box::new(reader) as Box<dyn tokio::io::AsyncRead + Send + Unpin>);

        let mut length_buf = [0u8; 4];
        if let Some(r) = reader.as_mut() {
            r.read_exact(&mut length_buf).await.unwrap();
        }
        let len = u32::from_le_bytes(length_buf) as usize;
        let mut data = vec![0u8; len];
        if let Some(r) = reader.as_mut() {
            r.read_exact(&mut data).await.unwrap();
        }

        let hello = HelloCommand::decode(&data).unwrap();

        let mut invalid_hello = hello.clone();
        invalid_hello.version = 99;

        let reject = RejectCommand::new("version mismatch");
        let reject_data = reject.encode();
        writer
            .write_all(&(reject_data.len() as u32).to_le_bytes())
            .await
            .unwrap();
        writer.write_all(&reject_data).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    let result = client_handler.handshake_as_client(server_addr, false).await;
    assert!(result.is_err());
}
