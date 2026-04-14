#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    use aex::connection::commands::{AckCommand, HelloCommand, RejectCommand, WelcomeCommand};
    use aex::connection::handshake_handler::HandshakeHandler;
    use aex::connection::node::Node;

    fn create_test_node() -> Node {
        Node::from_addr(
            "127.0.0.1:8080".parse().unwrap(),
            Some(1),
            Some(vec![1; 32]),
        )
    }

    #[test]
    fn test_handshake_handler_new() {
        let node = create_test_node();
        let handler = HandshakeHandler::new(node.clone());

        assert_eq!(handler.local_node, node);
        assert!(handler.session_keys.is_none());
        assert!(handler.on_established.is_none());
        assert!(handler.on_rejected.is_none());
    }

    #[test]
    fn test_handshake_handler_with_session_keys() {
        use aex::crypto::session_key_manager::PairedSessionKey;

        let node = create_test_node();
        let session_keys = PairedSessionKey::new(32);
        let keys = Arc::new(Mutex::new(session_keys));

        let handler = HandshakeHandler::new(node).with_session_keys(keys);
        assert!(handler.session_keys.is_some());
    }

    #[test]
    fn test_handshake_handler_on_established() {
        let node = create_test_node();
        let called = std::sync::Arc::new(std::sync::Mutex::new(false));
        let called_clone = called.clone();

        let handler = HandshakeHandler::new(node).on_established(move |_node, _addr| {
            *called_clone.lock().unwrap() = true;
        });

        assert!(handler.on_established.is_some());
    }

    #[test]
    fn test_handshake_handler_on_rejected() {
        let node = create_test_node();
        let called = std::sync::Arc::new(std::sync::Mutex::new(false));
        let called_clone = called.clone();

        let handler = HandshakeHandler::new(node).on_rejected(move |_reason, _addr| {
            *called_clone.lock().unwrap() = true;
        });

        assert!(handler.on_rejected.is_some());
    }

    #[test]
    fn test_handshake_handler_create_hello_without_encryption() {
        let node = create_test_node();
        let handler = HandshakeHandler::new(node);

        let hello = handler.create_hello(false);

        assert_eq!(hello.version, 1);
        assert!(!hello.request_encryption);
        assert!(hello.ephemeral_public.is_none());
    }

    #[test]
    fn test_handshake_handler_create_hello_with_encryption_no_keys() {
        let node = create_test_node();
        let handler = HandshakeHandler::new(node);

        let hello = handler.create_hello(true);

        assert!(hello.request_encryption);
        assert!(hello.ephemeral_public.is_none());
    }

    #[test]
    fn test_handshake_handler_create_hello_with_encryption_and_keys() {
        use aex::crypto::session_key_manager::PairedSessionKey;

        let node = create_test_node();
        let session_keys = PairedSessionKey::new(32);
        let keys = Arc::new(Mutex::new(session_keys));

        let handler = HandshakeHandler::new(node).with_session_keys(keys);

        let hello = handler.create_hello(true);

        assert!(hello.request_encryption);
        assert!(hello.ephemeral_public.is_some());
    }

    #[test]
    fn test_handshake_handler_create_welcome_accepted() {
        let node = create_test_node();
        let handler = HandshakeHandler::new(node);

        let welcome = handler.create_welcome(true, Some(vec![1, 2, 3]));

        assert!(welcome.accepted);
        assert_eq!(welcome.version, 1);
        assert!(welcome.ephemeral_public.is_some());
    }

    #[test]
    fn test_handshake_handler_create_welcome_rejected() {
        let node = create_test_node();
        let handler = HandshakeHandler::new(node);

        let welcome = handler.create_welcome(false, None);

        assert!(!welcome.accepted);
        assert!(welcome.ephemeral_public.is_none());
    }

    #[test]
    fn test_handshake_handler_create_ack() {
        let handler = HandshakeHandler::new(create_test_node());

        let ack = handler.create_ack(Some(vec![1, 2, 3]));

        assert!(ack.accepted);
        assert_eq!(ack.session_key_id, Some(vec![1, 2, 3]));
    }

    #[test]
    fn test_handshake_handler_create_reject() {
        let handler = HandshakeHandler::new(create_test_node());

        let reject = handler.create_reject("test reason");

        assert_eq!(reject.reason, "test reason");
    }
}
