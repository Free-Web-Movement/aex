use std::net::SocketAddr;

use aex::connection::handshake::{HandshakeContext, HandshakeState};
use aex::connection::node::Node;

#[test]
fn test_handshake_state() {
    let node = Node::from_system(8080, vec![0x78u8; 32], 1);
    let mut state = HandshakeState::new(node);

    let peer_addr: SocketAddr = "192.168.1.100:9000".parse().unwrap();
    let ctx = state.get_or_create(peer_addr);

    assert_eq!(ctx.peer_addr, peer_addr);
    assert!(!ctx.confirmed);
}

#[test]
fn test_handshake_state_remove() {
    let node = Node::from_system(8080, vec![0x79u8; 32], 1);
    let mut state = HandshakeState::new(node);

    let peer_addr: SocketAddr = "192.168.1.101:9001".parse().unwrap();
    state.get_or_create(peer_addr);
    assert!(state.peers.contains_key(&peer_addr));

    state.remove(&peer_addr);
    assert!(!state.peers.contains_key(&peer_addr));
}

#[test]
fn test_handshake_context() {
    let node = Node::from_system(8080, vec![0x7Au8; 32], 1);
    let peer_addr: SocketAddr = "192.168.1.102:9002".parse().unwrap();

    let ctx = HandshakeContext::new(node.clone(), peer_addr);

    assert_eq!(ctx.local_node.id, node.id);
    assert_eq!(ctx.peer_addr, peer_addr);
    assert!(!ctx.encryption_enabled);
    assert!(!ctx.confirmed);
}

#[test]
fn test_handshake_context_with_encryption() {
    let node = Node::from_system(8080, vec![0x7Bu8; 32], 1);
    let peer_addr: SocketAddr = "192.168.1.103:9003".parse().unwrap();

    let ctx = HandshakeContext::new(node, peer_addr).with_encryption(true);

    assert!(ctx.encryption_enabled);
}

#[test]
fn test_handshake_context_set_peer_node() {
    let local_node = Node::from_system(8080, vec![0x7Cu8; 32], 1);
    let peer_node = Node::from_system(9090, vec![0x7Du8; 32], 1);
    let peer_addr: SocketAddr = "192.168.1.104:9004".parse().unwrap();

    let mut ctx = HandshakeContext::new(local_node, peer_addr);
    ctx.set_peer_node(peer_node.clone());

    assert_eq!(ctx.peer_node, Some(peer_node));
}

#[test]
fn test_handshake_context_confirm() {
    let node = Node::from_system(8080, vec![0x7Eu8; 32], 1);
    let peer_addr: SocketAddr = "192.168.1.105:9005".parse().unwrap();
    let session_id = vec![0x7Fu8; 16];

    let mut ctx = HandshakeContext::new(node, peer_addr);
    ctx.confirm(Some(session_id.clone()));

    assert!(ctx.confirmed);
    assert_eq!(ctx.session_key_id, Some(session_id));
}
