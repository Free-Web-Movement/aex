use std::collections::HashMap;
use std::net::SocketAddr;

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
