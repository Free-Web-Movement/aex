#[cfg(test)]
mod tests {
    #[test]
    fn test_macro_usable_via_on() {
        use aex::tcp::router::Router as TcpRouter;
        use aex::tcp::types::{Command, RawCodec};
        
        let mut router = TcpRouter::<RawCodec, RawCodec>::new().extractor(|c: &RawCodec| c.id());
        router.on::<RawCodec, RawCodec>(
            1,
            Box::new(|_ctx, _frame: RawCodec, _cmd: RawCodec| Box::pin(async { Ok(true) })),
            vec![],
        );
        
        assert!(router.handlers.contains_key(&1));
    }
}