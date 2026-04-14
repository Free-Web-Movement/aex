#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
    use std::sync::Arc;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    use aex::udp::router::Router;
    use aex::connection::global::GlobalContext;
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
    fn test_udp_router_new() {
        let router = Router::new();
        assert!(router.handlers.is_empty());
    }

    #[test]
    fn test_udp_router_on() {
        let mut router = Router::new();

        router.on::<TestUdpFrame, TestUdpCommand, _, _>(
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
        let mut router = Router::new();

        router.on::<TestUdpFrame, TestUdpCommand, _, _>(
            1,
            |_global, _frame, _cmd, _addr, _socket| {
                Box::pin(async { Ok(true) })
            },
        );

        router.on::<TestUdpFrame, TestUdpCommand, _, _>(
            2,
            |_global, _frame, _cmd, _addr, _socket| {
                Box::pin(async { Ok(true) })
            },
        );

        router.on::<TestUdpFrame, TestUdpCommand, _, _>(
            3,
            |_global, _frame, _cmd, _addr, _socket| {
                Box::pin(async { Ok(false) })
            },
        );

        assert_eq!(router.handlers.len(), 3);
    }

    #[test]
    fn test_udp_router_handler_replacement() {
        let mut router = Router::new();

        router.on::<TestUdpFrame, TestUdpCommand, _, _>(
            100,
            |_global, _frame, _cmd, _addr, _socket| {
                Box::pin(async { Ok(true) })
            },
        );

        router.on::<TestUdpFrame, TestUdpCommand, _, _>(
            100,
            |_global, _frame, _cmd, _addr, _socket| {
                Box::pin(async { Ok(false) })
            },
        );

        assert_eq!(router.handlers.len(), 1);
    }

    #[test]
    fn test_udp_router_handler_downcast() {
        use std::any::Any;

        let mut router = Router::new();

        router.on::<TestUdpFrame, TestUdpCommand, _, _>(
            100,
            |_global, _frame, _cmd, _addr, _socket| {
                Box::pin(async { Ok(true) })
            },
        );

        let handler = router.handlers.get(&100).unwrap();
        let handler_ref = handler.downcast_ref::<Box<dyn Fn(
            Arc<GlobalContext>,
            TestUdpFrame,
            TestUdpCommand,
            SocketAddr,
            Arc<tokio::net::UdpSocket>,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<bool>> + Send>> + Send + Sync>>();

        assert!(handler_ref.is_some());
    }

    #[test]
    fn test_udp_router_handler_arc() {
        let router = Router::new();
        let arc_router = Arc::new(router);
        
        // Arc<Self> should work with handle method
        assert_eq!(arc_router.handlers.len(), 0);
    }

    #[tokio::test]
    async fn test_udp_router_handler_traits() {
        // Test that the handler implements Send + Sync
        let mut router = Router::new();

        let handler: Box<dyn Fn(
            Arc<GlobalContext>,
            TestUdpFrame,
            TestUdpCommand,
            SocketAddr,
            Arc<tokio::net::UdpSocket>,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<bool>> + Send>> + Send + Sync> = 
            Box::new(|_global, _frame, _cmd, _addr, _socket| {
                Box::pin(async { Ok(true) })
            });

        // Verify it can be stored in router
        router.on::<TestUdpFrame, TestUdpCommand, _, _>(1, move |_, _, _, _, _| {
            Box::pin(async { Ok(true) })
        });

        // Verify Send (required for Arc<Self>)
        fn assert_send<T: Send>(_: &T) {}
        assert_send(&router);
        
        // Verify Sync
        fn assert_sync<T: Sync>(_: &T) {}
        assert_sync(&router);
    }

    #[test]
    fn test_udp_router_frame_with_different_commands() {
        // Test different command IDs
        let mut router = Router::new();

        router.on::<TestUdpFrame, TestUdpCommand, _, _>(
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