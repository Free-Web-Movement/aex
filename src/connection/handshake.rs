use std::collections::HashMap;
use std::net::SocketAddr;

use crate::connection::commands::{
    AckCommand, HelloCommand, RejectCommand, WelcomeCommand, CMD_ACK, CMD_HELLO, CMD_REJECT,
    CMD_WELCOME,
};
use crate::connection::node::Node;

pub const HANDSHAKE_VERSION: u8 = 1;

pub struct HandshakeContext {
    pub local_node: Node,
    pub peer_node: Option<Node>,
    pub peer_addr: SocketAddr,
    pub encryption_enabled: bool,
    pub session_key_id: Option<Vec<u8>>,
    pub confirmed: bool,
}

impl HandshakeContext {
    pub fn new(local_node: Node, peer_addr: SocketAddr) -> Self {
        Self {
            local_node,
            peer_node: None,
            peer_addr,
            encryption_enabled: false,
            session_key_id: None,
            confirmed: false,
        }
    }

    pub fn with_encryption(mut self, enabled: bool) -> Self {
        self.encryption_enabled = enabled;
        self
    }

    pub fn set_peer_node(&mut self, node: Node) {
        self.peer_node = Some(node);
    }

    pub fn confirm(&mut self, session_key_id: Option<Vec<u8>>) {
        self.session_key_id = session_key_id;
        self.confirmed = true;
    }
}

pub struct HandshakeState {
    pub local: Node,
    pub peers: HashMap<SocketAddr, HandshakeContext>,
}

impl HandshakeState {
    pub fn new(local: Node) -> Self {
        Self {
            local,
            peers: HashMap::new(),
        }
    }

    pub fn get_or_create(&mut self, peer_addr: SocketAddr) -> &mut HandshakeContext {
        self.peers
            .entry(peer_addr)
            .or_insert_with(|| HandshakeContext::new(self.local.clone(), peer_addr))
    }

    pub fn remove(&mut self, peer_addr: &SocketAddr) {
        self.peers.remove(peer_addr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handshake_state() {
        let node = Node::from_system(8080, vec![0x78u8; 32], 1);
        let mut state = HandshakeState::new(node);

        let peer_addr: SocketAddr = "192.168.1.100:9000".parse().unwrap();
        let ctx = state.get_or_create(peer_addr);

        assert_eq!(ctx.peer_addr, peer_addr);
        assert!(!ctx.confirmed);
    }
}
