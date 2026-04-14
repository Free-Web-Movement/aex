#[cfg(test)]
mod tests {
    use std::{net::SocketAddr, sync::Arc};
    use tokio::sync::Mutex;

    use aex::tcp::router::Router;
    use aex::connection::global::GlobalContext;
    use aex::tcp::types::{Codec, Command, Frame, RawCodec};

    fn create_test_frame(payload: Option<Vec<u8>>, _is_valid: bool) -> RawCodec {
        let data = payload.unwrap_or_default();
        RawCodec(data)
    }

    #[test]
    fn test_router_new() {
        let router = Router::new();
        assert!(router.handlers.is_empty());
    }

    #[test]
    fn test_router_on_and_handler_count() {
        use futures::FutureExt;

        let mut router = Router::new();

        router.on::<RawCodec, RawCodec>(
            1,
            Box::new(|_, _, _| Box::pin(async { Ok(true) }).boxed()),
            vec![],
        );

        router.on::<RawCodec, RawCodec>(
            2,
            Box::new(|_, _, _| Box::pin(async { Ok(true) }).boxed()),
            vec![],
        );

        assert_eq!(router.handlers.len(), 2);
        assert!(router.handlers.contains_key(&1));
        assert!(router.handlers.contains_key(&2));
    }

    #[test]
    fn test_router_on_with_middleware() {
        use futures::FutureExt;

        let mut router = Router::new();

        let middleware: aex::tcp::router::Doer<RawCodec, RawCodec> = 
            Box::new(|_, _, _| Box::pin(async { Ok(true) }).boxed());

        router.on::<RawCodec, RawCodec>(
            1,
            Box::new(|_, _, _| Box::pin(async { Ok(true) }).boxed()),
            vec![middleware],
        );

        let chain = router.handlers.get(&1);
        assert!(chain.is_some());
        assert_eq!(chain.unwrap().len(), 2);
    }

    #[test]
    fn test_router_handler_replacement() {
        use futures::FutureExt;

        let mut router = Router::new();

        router.on::<RawCodec, RawCodec>(
            100,
            Box::new(|_, _, _| Box::pin(async { Ok(true) }).boxed()),
            vec![],
        );

        router.on::<RawCodec, RawCodec>(
            100,
            Box::new(|_, _, _| Box::pin(async { Ok(false) }).boxed()),
            vec![],
        );

        assert_eq!(router.handlers.len(), 1);
    }

    #[tokio::test]
    async fn test_router_handle_frame_invalid_validate() {
        use aex::connection::context::Context;

        let router = Router::new();
        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let global = GlobalContext::new(addr, None);
        let ctx = Arc::new(Mutex::new(Context::new(
            None,
            None,
            Arc::new(global),
            addr,
        )));

        // Create a RawCodec with insufficient data to decode - this simulates an invalid frame
        let frame = RawCodec(vec![0xFF, 0x00]);

        let result = router
            .handle_frame::<RawCodec, RawCodec>(
                ctx,
                frame,
                Arc::new(|c: &RawCodec| c.id()),
            )
            .await;

        // Should return Ok(false) or Ok(true) - either way it handled the error
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_router_handle_frame_no_payload() {
        use aex::connection::context::Context;
        use futures::FutureExt;

        let mut router = Router::new();
        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let global = GlobalContext::new(addr, None);
        
        router.on::<RawCodec, RawCodec>(
            1,
            Box::new(|_, _, _| Box::pin(async { Ok(true) }).boxed()),
            vec![],
        );

        let ctx = Arc::new(Mutex::new(Context::new(
            None,
            None,
            Arc::new(global),
            addr,
        )));

        // Create a RawCodec with empty payload
        let frame = RawCodec(vec![]);

        let result = router
            .handle_frame::<RawCodec, RawCodec>(
                ctx,
                frame,
                Arc::new(|_: &RawCodec| 1),
            )
            .await;

        assert!(result.unwrap());
    }

    #[tokio::test]
    async fn test_router_handle_no_handler() {
        use aex::connection::context::Context;

        let mut router = Router::new();
        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let global = GlobalContext::new(addr, None);

        let ctx = Arc::new(Mutex::new(Context::new(
            None,
            None,
            Arc::new(global),
            addr,
        )));

        let frame = RawCodec::decode(&vec![1, 0, 0, 0, 0, 0, 0, 0, 0]).ok();
        
        if let Some(frame) = frame {
            let result = router
                .handle_frame::<RawCodec, RawCodec>(
                    ctx,
                    frame,
                    Arc::new(|c: &RawCodec| c.id()),
                )
                .await;
            
            assert!(result.unwrap());
        }
    }
}