#[cfg(test)]
mod tests {
    use aex::connection::commands::{
        ack::AckCommand, command_id::CommandId, hello::HelloCommand, ping::PingCommand,
        reject::RejectCommand, welcome::WelcomeCommand,
    };
    use aex::connection::node::Node;

    fn create_test_node() -> Node {
        Node::from_addr(
            "127.0.0.1:8080".parse().unwrap(),
            Some(1),
            Some(vec![1; 32]),
        )
    }

    #[test]
    fn test_command_id_from_u32_hello() {
        assert_eq!(CommandId::from_u32(1), Some(CommandId::Hello));
    }

    #[test]
    fn test_command_id_from_u32_welcome() {
        assert_eq!(CommandId::from_u32(2), Some(CommandId::Welcome));
    }

    #[test]
    fn test_command_id_from_u32_ack() {
        assert_eq!(CommandId::from_u32(3), Some(CommandId::Ack));
    }

    #[test]
    fn test_command_id_from_u32_reject() {
        assert_eq!(CommandId::from_u32(4), Some(CommandId::Reject));
    }

    #[test]
    fn test_command_id_from_u32_ping() {
        assert_eq!(CommandId::from_u32(5), Some(CommandId::Ping));
    }

    #[test]
    fn test_command_id_from_u32_pong() {
        assert_eq!(CommandId::from_u32(6), Some(CommandId::Pong));
    }

    #[test]
    fn test_command_id_from_u32_invalid() {
        assert_eq!(CommandId::from_u32(0), None);
        assert_eq!(CommandId::from_u32(7), None);
        assert_eq!(CommandId::from_u32(100), None);
    }

    #[test]
    fn test_command_id_as_u32() {
        assert_eq!(CommandId::Hello.as_u32(), 1);
        assert_eq!(CommandId::Welcome.as_u32(), 2);
        assert_eq!(CommandId::Ack.as_u32(), 3);
        assert_eq!(CommandId::Reject.as_u32(), 4);
        assert_eq!(CommandId::Ping.as_u32(), 5);
        assert_eq!(CommandId::Pong.as_u32(), 6);
    }

    #[test]
    fn test_command_id_into_u32() {
        let id: u32 = CommandId::Hello.into();
        assert_eq!(id, 1);
    }

    #[test]
    fn test_command_id_traits() {
        let id1 = CommandId::Hello;
        let id2 = CommandId::Hello;
        let id3 = CommandId::Welcome;

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);

        let _ = format!("{:?}", id1);
        let _ = id1.clone();
    }

    #[test]
    fn test_hello_command_new() {
        let node = create_test_node();
        let hello = HelloCommand::new(node.clone(), Some(vec![1, 2, 3]), true);

        assert_eq!(hello.version, 1);
        assert_eq!(hello.node, node);
        assert_eq!(hello.ephemeral_public, Some(vec![1, 2, 3]));
        assert!(hello.request_encryption);
    }

    #[test]
    fn test_hello_command_id() {
        assert_eq!(HelloCommand::id(), CommandId::Hello);
    }

    #[test]
    fn test_hello_command_is_valid() {
        let node = create_test_node();
        let hello = HelloCommand::new(node, None, false);
        assert!(hello.is_valid());
    }

    #[test]
    fn test_hello_command_encode() {
        let node = create_test_node();
        let hello = HelloCommand::new(node, None, false);
        let encoded = hello.encode();

        assert!(encoded.len() > 4);
        let id = u32::from_le_bytes([encoded[0], encoded[1], encoded[2], encoded[3]]);
        assert_eq!(id, 1);
    }

    #[test]
    fn test_hello_command_decode() {
        let node = create_test_node();
        let hello = HelloCommand::new(node, None, false);
        let encoded = hello.encode();

        let decoded = HelloCommand::decode(&encoded).unwrap();
        assert_eq!(decoded.version, 1);
    }

    #[test]
    fn test_hello_command_decode_invalid_id() {
        let mut data = vec![0u8; 4];
        data.extend_from_slice(b"invalid");
        let result = HelloCommand::decode(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_hello_command_decode_too_short() {
        let result = HelloCommand::decode(b"");
        assert!(result.is_err());
    }

    #[test]
    fn test_welcome_command_new() {
        let node = create_test_node();
        let welcome = WelcomeCommand::new(node.clone(), true, Some(vec![1, 2, 3]));

        assert_eq!(welcome.version, 1);
        assert_eq!(welcome.node, node);
        assert!(welcome.accepted);
        assert_eq!(welcome.ephemeral_public, Some(vec![1, 2, 3]));
    }

    #[test]
    fn test_welcome_command_rejected() {
        let welcome = WelcomeCommand::rejected();

        assert_eq!(welcome.version, 1);
        assert!(!welcome.accepted);
        assert!(welcome.ephemeral_public.is_none());
    }

    #[test]
    fn test_welcome_command_id() {
        assert_eq!(WelcomeCommand::id(), CommandId::Welcome);
    }

    #[test]
    fn test_welcome_command_is_valid() {
        let node = create_test_node();
        let welcome = WelcomeCommand::new(node, true, None);
        assert!(welcome.is_valid());
    }

    #[test]
    fn test_welcome_command_encode_decode() {
        let node = create_test_node();
        let welcome = WelcomeCommand::new(node, true, Some(vec![1, 2, 3]));
        let encoded = welcome.encode();

        let decoded = WelcomeCommand::decode(&encoded).unwrap();
        assert!(decoded.accepted);
    }

    #[test]
    fn test_ack_command_accepted() {
        let ack = AckCommand::accepted(Some(vec![1, 2, 3]));

        assert!(ack.accepted);
        assert_eq!(ack.session_key_id, Some(vec![1, 2, 3]));
    }

    #[test]
    fn test_ack_command_rejected() {
        let ack = AckCommand::rejected();

        assert!(!ack.accepted);
        assert!(ack.session_key_id.is_none());
    }

    #[test]
    fn test_ack_command_id() {
        assert_eq!(AckCommand::id(), CommandId::Ack);
    }

    #[test]
    fn test_ack_command_encode_decode() {
        let ack = AckCommand::accepted(Some(vec![1, 2, 3]));
        let encoded = ack.encode();

        let decoded = AckCommand::decode(&encoded).unwrap();
        assert!(decoded.accepted);
    }

    #[test]
    fn test_reject_command_new() {
        let reject = RejectCommand::new("test reason");

        assert_eq!(reject.reason, "test reason");
    }

    #[test]
    fn test_reject_command_id() {
        assert_eq!(RejectCommand::id(), CommandId::Reject);
    }

    #[test]
    fn test_reject_command_encode_decode() {
        let reject = RejectCommand::new("connection refused");
        let encoded = reject.encode();

        let decoded = RejectCommand::decode(&encoded).unwrap();
        assert_eq!(decoded.reason, "connection refused");
    }

    #[test]
    fn test_ping_command_new() {
        let ping = PingCommand::new();

        assert!(ping.timestamp > 0);
        assert!(ping.nonce.is_none());
    }

    #[test]
    fn test_ping_command_with_nonce() {
        let ping = PingCommand::with_nonce(vec![1, 2, 3]);

        assert!(ping.nonce.is_some());
        assert_eq!(ping.nonce.unwrap(), vec![1, 2, 3]);
    }

    #[test]
    fn test_ping_command_default() {
        let ping = PingCommand::default();
        assert!(ping.timestamp > 0);
    }

    #[test]
    fn test_ping_command_id() {
        assert_eq!(PingCommand::id(), CommandId::Ping);
    }

    #[test]
    fn test_ping_command_encode_decode() {
        let ping = PingCommand::with_nonce(vec![1, 2, 3]);
        let encoded = ping.encode();

        let decoded = PingCommand::decode(&encoded).unwrap();
        assert!(decoded.nonce.is_some());
    }

    #[test]
    fn test_pong_command_new() {
        let pong = aex::connection::commands::ping::PongCommand::new(12345, Some(vec![1, 2]));

        assert_eq!(pong.timestamp, 12345);
        assert_eq!(pong.nonce, Some(vec![1, 2]));
        assert!(pong.local_time > 0);
    }

    #[test]
    fn test_pong_command_latency() {
        let timestamp = 1000;
        let local_time = 1100;
        let pong = aex::connection::commands::ping::PongCommand::new(timestamp, None);

        // Use internal field access
        assert!(pong.local_time >= timestamp);
    }

    #[test]
    fn test_pong_command_id() {
        assert_eq!(
            aex::connection::commands::ping::PongCommand::id(),
            CommandId::Pong
        );
    }

    #[test]
    fn test_pong_command_encode_decode() {
        let pong = aex::connection::commands::ping::PongCommand::new(12345, Some(vec![1, 2]));
        let encoded = pong.encode();

        let decoded = aex::connection::commands::ping::PongCommand::decode(&encoded).unwrap();
        assert_eq!(decoded.timestamp, 12345);
    }

    #[test]
    fn test_detect_command_type() {
        use aex::connection::commands::ping::detect_command_type;

        let hello_data = {
            let cmd = HelloCommand::new(create_test_node(), None, false);
            cmd.encode()
        };
        assert_eq!(detect_command_type(&hello_data), Some(CommandId::Hello));

        let ping_data = PingCommand::new().encode();
        assert_eq!(detect_command_type(&ping_data), Some(CommandId::Ping));
    }

    #[test]
    fn test_detect_command_type_too_short() {
        use aex::connection::commands::ping::detect_command_type;

        assert_eq!(detect_command_type(b""), None);
        assert_eq!(detect_command_type(b"abc"), None);
    }

    #[test]
    fn test_detect_command_type_invalid_id() {
        use aex::connection::commands::ping::detect_command_type;

        let mut data = [0u8; 4];
        data.copy_from_slice(&99u32.to_le_bytes());
        assert_eq!(detect_command_type(&data), None);
    }
}
