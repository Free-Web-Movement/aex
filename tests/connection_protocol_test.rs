#[cfg(test)]
mod tests {
    use aex::connection::protocol::Protocol;
    use serde_json;

    #[test]
    fn test_protocol_as_str_exhaustive() {
        let cases = [
            (Protocol::Tcp, "tcp"),
            (Protocol::Udp, "udp"),
            (Protocol::Http, "http"),
            (Protocol::Ws, "ws"),
            (Protocol::Custom("libp2p".to_string()), "libp2p"),
        ];

        for (proto, expected) in cases {
            // 覆盖 as_str 的每一个 match 分支
            assert_eq!(proto.as_str(), expected);
        }
    }

    #[test]
    fn test_protocol_from_str_logic() {
        // 覆盖标准变体
        assert_eq!(Protocol::from("tcp"), Protocol::Tcp);
        assert_eq!(Protocol::from("UDP"), Protocol::Udp); // 测试 to_lowercase
        assert_eq!(Protocol::from("Http"), Protocol::Http);
        assert_eq!(Protocol::from("ws"), Protocol::Ws);

        // 覆盖 Custom 分支
        let custom = Protocol::from("quic");
        if let Protocol::Custom(s) = custom {
            assert_eq!(s, "quic");
        } else {
            panic!("Should be Protocol::Custom");
        }
    }

    #[test]
    fn test_protocol_serde_roundtrip() {
        // 覆盖序列化与反序列化，特别是 Custom 变体
        let protocols = vec![
            Protocol::Tcp,
            Protocol::Custom("grpc".to_string()),
        ];

        let serialized = serde_json::to_string(&protocols).unwrap();
        // 验证 JSON 格式是否符合预期
        assert!(serialized.contains("\"Tcp\""));
        assert!(serialized.contains("{\"Custom\":\"grpc\"}"));

        let deserialized: Vec<Protocol> = serde_json::from_str(&serialized).unwrap();
        assert_eq!(protocols, deserialized);
    }

    #[test]
    fn test_derived_traits() {
        // 覆盖 Clone, PartialEq, Hash
        let p1 = Protocol::Custom("p2p".to_string());
        let p2 = p1.clone();
        assert_eq!(p1, p2);
        
        // 覆盖 Debug
        assert!(format!("{:?}", p1).contains("Custom"));
    }
}