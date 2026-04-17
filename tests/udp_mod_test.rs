#[cfg(test)]
mod tests {
    use aex::udp::router::Router as UdpRouter;
    use aex::tcp::types::{Codec, Command, Frame};
    use bincode::{Decode, Encode};
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
    struct TestUdpFrame {
        payload: Option<Vec<u8>>,
        is_valid: bool,
    }

    impl Frame for TestUdpFrame {
        fn payload(&self) -> Option<Vec<u8>> {
            self.payload.clone()
        }
        fn validate(&self) -> bool {
            self.is_valid
        }
        fn command(&self) -> Option<&Vec<u8>> {
            self.payload.as_ref()
        }
        fn is_flat(&self) -> bool {
            false
        }
    }

    impl Codec for TestUdpFrame {}

    #[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
    struct TestUdpCommand {
        id: u32,
        valid: bool,
        data: Vec<u8>,
    }

    impl Command for TestUdpCommand {
        fn id(&self) -> u32 {
            self.id
        }
        fn validate(&self) -> bool {
            self.valid
        }
        fn data(&self) -> &Vec<u8> {
            &self.data
        }
    }

    impl Codec for TestUdpCommand {}

    #[test]
    fn test_udp_router_creation() {
        let router = UdpRouter::<TestUdpFrame, TestUdpCommand>::new();
        assert!(router.handlers.is_empty());
    }

    #[test]
    fn test_udp_exports() {
        let _ = UdpRouter::<TestUdpFrame, TestUdpCommand>::new();
    }

    #[tokio::test]
    async fn test_udp_socket_binding() {
        let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let local_addr = socket.local_addr().unwrap();
        assert!(local_addr.port() > 0);
    }

    #[test]
    fn test_udp_router_with_extractor() {
        let router = UdpRouter::<TestUdpFrame, TestUdpCommand>::new()
            .extractor(|c: &TestUdpCommand| c.id());
        assert!(router.get_extractor().is_some());
    }

    #[test]
    fn test_udp_router_on() {
        let mut router = UdpRouter::<TestUdpFrame, TestUdpCommand>::new()
            .extractor(|c: &TestUdpCommand| c.id());

        router.on(
            100,
            |_global, _frame, _cmd, _addr, _socket| {
                Box::pin(async { Ok(true) })
            },
        );

        assert_eq!(router.handlers.len(), 1);
        assert!(router.handlers.contains_key(&100));
    }
}
