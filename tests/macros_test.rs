#[cfg(test)]
mod tests {
    #[test]
    fn test_macro_usable_via_on() {
        use aex::tcp::router::Router as TcpRouter;
        
        let mut router = TcpRouter::new();
        router.on::<aex::tcp::types::RawCodec, aex::tcp::types::RawCodec>(
            1,
            Box::new(|_ctx, _frame, _cmd| Box::pin(async { Ok(true) })),
            vec![],
        );
        
        assert!(router.handlers.contains_key(&1));
    }
}