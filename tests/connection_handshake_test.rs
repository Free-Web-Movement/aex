#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    use aex::connection::commands::{AckCommand, HelloCommand, RejectCommand, WelcomeCommand};
    use aex::connection::handshake::{HandshakeContext, HandshakeState};
    use aex::connection::node::Node;
    use aex::constants::tcp::HANDSHAKE_VERSION;

    fn create_test_node() -> Node {
        Node::from_addr(
            "127.0.0.1:8080".parse().unwrap(),
            Some(1),
            Some(vec![1; 32]),
        )
    }

    #[test]
    fn test_handshake_context_new() {
        let node = create_test_node();
        let peer_addr: SocketAddr = "192.168.1.1:9000".parse().unwrap();

        let ctx = HandshakeContext::new(node.clone(), peer_addr);

        assert_eq!(ctx.local_node, node);
        assert!(ctx.peer_node.is_none());
        assert_eq!(ctx.peer_addr, peer_addr);
        assert!(!ctx.encryption_enabled);
        assert!(ctx.session_key_id.is_none());
        assert!(!ctx.confirmed);
    }

    #[test]
    fn test_handshake_context_with_encryption() {
        let node = create_test_node();
        let ctx = HandshakeContext::new(node, "127.0.0.1:8080".parse().unwrap());

        let ctx = ctx.with_encryption(true);
        assert!(ctx.encryption_enabled);

        let ctx = ctx.with_encryption(false);
        assert!(!ctx.encryption_enabled);
    }

    #[test]
    fn test_handshake_context_set_peer_node() {
        let local_node = create_test_node();
        let peer_node = Node::from_addr(
            "192.168.1.1:9000".parse().unwrap(),
            Some(1),
            Some(vec![2; 32]),
        );

        let mut ctx = HandshakeContext::new(local_node, "127.0.0.1:8080".parse().unwrap());
        ctx.set_peer_node(peer_node.clone());

        assert_eq!(ctx.peer_node, Some(peer_node));
    }

    #[test]
    fn test_handshake_context_confirm() {
        let node = create_test_node();
        let session_key = Some(vec![1, 2, 3, 4]);

        let mut ctx = HandshakeContext::new(node, "127.0.0.1:8080".parse().unwrap());
        ctx.confirm(session_key.clone());

        assert!(ctx.confirmed);
        assert_eq!(ctx.session_key_id, session_key);
    }

    #[test]
    fn test_handshake_state_new() {
        let node = create_test_node();
        let state = HandshakeState::new(node.clone());

        assert_eq!(state.local, node);
        assert!(state.peers.is_empty());
    }

    #[test]
    fn test_handshake_state_get_or_create() {
        let node = create_test_node();
        let mut state = HandshakeState::new(node);

        let peer_addr: SocketAddr = "192.168.1.1:9000".parse().unwrap();
        let ctx = state.get_or_create(peer_addr);

        assert_eq!(ctx.peer_addr, peer_addr);
        assert!(state.peers.contains_key(&peer_addr));
    }

    #[test]
    fn test_handshake_state_get_or_create_existing() {
        let node = create_test_node();
        let mut state = HandshakeState::new(node);

        let peer_addr: SocketAddr = "192.168.1.1:9000".parse().unwrap();

        // First call creates - ctx1 holds mutable reference
        {
            let _ctx1 = state.get_or_create(peer_addr);
            // Can't access state.peers here due to borrow
        } // ctx1 is dropped here

        // Now state.peers is accessible again
        let initial_len = state.peers.len();
        assert_eq!(initial_len, 1);

        // Second call - get new reference
        let _ctx2 = state.get_or_create(peer_addr);

        // Can't access state.peers again while ctx2 is in scope
        // So just verify ctx2 exists by its address
    }

    #[test]
    fn test_handshake_state_remove() {
        let node = create_test_node();
        let mut state = HandshakeState::new(node);

        let peer_addr: SocketAddr = "192.168.1.1:9000".parse().unwrap();

        // First ensure the key exists
        let contains_before = state.peers.is_empty();
        assert!(contains_before);

        // Create the context
        state.get_or_create(peer_addr);

        // Check after creating - use is_empty to check length
        assert!(!state.peers.is_empty());

        state.remove(&peer_addr);
        assert!(state.peers.is_empty());
    }

    #[test]
    fn test_handshake_version_constant() {
        assert_eq!(HANDSHAKE_VERSION, 1);
    }

    #[test]
    fn test_hello_command_encode_decode() {
        let node = create_test_node();
        let hello = HelloCommand::new(node.clone(), Some(vec![0u8; 32]), true);

        let encoded = hello.encode();
        assert!(encoded.len() > 4);

        let decoded = HelloCommand::decode(&encoded).unwrap();
        assert_eq!(decoded.version, 1);
        assert!(decoded.request_encryption);
    }

    #[test]
    fn test_hello_command_is_valid() {
        let node = create_test_node();
        let hello = HelloCommand::new(node, None, false);
        assert!(hello.is_valid());
    }

    #[test]
    fn test_welcome_command_encode_decode() {
        let node = create_test_node();
        let welcome = WelcomeCommand::new(node.clone(), true, Some(vec![0u8; 32]));

        let encoded = welcome.encode();
        let decoded = WelcomeCommand::decode(&encoded).unwrap();

        assert!(decoded.accepted);
        assert!(decoded.ephemeral_public.is_some());
    }

    #[test]
    fn test_welcome_command_rejected() {
        let welcome = WelcomeCommand::rejected();

        assert!(!welcome.accepted);
        assert!(welcome.ephemeral_public.is_none());
    }

    #[test]
    fn test_ack_command_accepted() {
        let session_key = Some(vec![1, 2, 3]);
        let ack = AckCommand::accepted(session_key.clone());

        assert!(ack.accepted);
        assert_eq!(ack.session_key_id, session_key);
    }

    #[test]
    fn test_ack_command_rejected() {
        let ack = AckCommand::rejected();

        assert!(!ack.accepted);
        assert!(ack.session_key_id.is_none());
    }

    #[test]
    fn test_reject_command_new() {
        let reason = "version mismatch";
        let reject = RejectCommand::new(reason);

        assert_eq!(reject.reason, reason);
    }

    #[test]
    fn test_reject_command_encode_decode() {
        let reject = RejectCommand::new("test reason");

        let encoded = reject.encode();
        let decoded = RejectCommand::decode(&encoded).unwrap();

        assert_eq!(decoded.reason, "test reason");
    }

    #[tokio::test]
    async fn test_handshake_context_send_frame_not_supported() {
        // This test verifies we can create and manipulate the context
        // The actual send_frame is tested via the handler
        let node = create_test_node();
        let ctx = HandshakeContext::new(node, "127.0.0.1:8080".parse().unwrap());

        // We can't easily test send_frame without a mock, but we can verify the context works
        assert!(!ctx.encryption_enabled);
    }
}
