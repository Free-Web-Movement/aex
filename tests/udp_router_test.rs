#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use aex::udp::router::{Router as UdpRouter, UdpHandler};
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
    fn test_udp_router_new_with_handler() {
        let router = UdpRouter::<TestUdpFrame, TestUdpCommand>::new_with_handler();
        assert_eq!(router.handler_count(), 1);
    }

    #[test]
    fn test_udp_router_new() {
        let router = UdpRouter::<TestUdpFrame, TestUdpCommand>::new();
        assert!(router.handlers.is_empty());
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

    #[test]
    fn test_udp_router_on_multiple_handlers() {
        let mut router = UdpRouter::<TestUdpFrame, TestUdpCommand>::new()
            .extractor(|c: &TestUdpCommand| c.id());

        router.on(
            1,
            |_global, _frame, _cmd, _addr, _socket| {
                Box::pin(async { Ok(true) })
            },
        );

        router.on(
            2,
            |_global, _frame, _cmd, _addr, _socket| {
                Box::pin(async { Ok(true) })
            },
        );

        router.on(
            3,
            |_global, _frame, _cmd, _addr, _socket| {
                Box::pin(async { Ok(false) })
            },
        );

        assert_eq!(router.handlers.len(), 3);
    }

    #[test]
    fn test_udp_router_handler_replacement() {
        let mut router = UdpRouter::<TestUdpFrame, TestUdpCommand>::new()
            .extractor(|c: &TestUdpCommand| c.id());

        router.on(
            100,
            |_global, _frame, _cmd, _addr, _socket| {
                Box::pin(async { Ok(true) })
            },
        );

        router.on(
            100,
            |_global, _frame, _cmd, _addr, _socket| {
                Box::pin(async { Ok(false) })
            },
        );

        assert_eq!(router.handlers.len(), 1);
    }

    #[test]
    fn test_udp_router_handler_downcast() {
        let mut router = UdpRouter::<TestUdpFrame, TestUdpCommand>::new()
            .extractor(|c: &TestUdpCommand| c.id());

        router.on(
            100,
            |_global, _frame, _cmd, _addr, _socket| {
                Box::pin(async { Ok(true) })
            },
        );

        let handler = router.handlers.get(&100).unwrap();
        let _handler_ref = handler.downcast_ref::<Box<UdpHandler<TestUdpFrame, TestUdpCommand>>>();
    }

    #[test]
    fn test_udp_router_handler_arc() {
        let router = UdpRouter::<TestUdpFrame, TestUdpCommand>::new();
        let arc_router = Arc::new(router);
        
        assert_eq!(arc_router.handlers.len(), 0);
    }

    #[tokio::test]
    async fn test_udp_router_handler_traits() {
        let mut router = UdpRouter::<TestUdpFrame, TestUdpCommand>::new()
            .extractor(|c: &TestUdpCommand| c.id());

        router.on(1, move |_, _, _, _, _| {
            Box::pin(async { Ok(true) })
        });

        fn assert_send<T: Send>(_: &T) {}
        assert_send(&router);
        
        fn assert_sync<T: Sync>(_: &T) {}
        assert_sync(&router);
    }

    #[test]
    fn test_udp_router_frame_with_different_commands() {
        let mut router = UdpRouter::<TestUdpFrame, TestUdpCommand>::new()
            .extractor(|c: &TestUdpCommand| c.id());

        router.on(
            1,
            |_global, _frame, cmd, _addr, _socket| {
                Box::pin(async move {
                    println!("Command ID: {}", cmd.id());
                    Ok(true)
                })
            },
        );

        let cmd = TestUdpCommand {
            id: 1,
            valid: true,
            data: vec![1, 2, 3],
        };

        assert_eq!(cmd.id(), 1);
        assert!(cmd.validate());
    }
}
