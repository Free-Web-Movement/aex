#[cfg(test)]
mod tests {
    #[test]
    fn test_tcp_router_creation() {
        use aex::tcp::router::Router;
        use aex::tcp::types::RawCodec;

        let router: Router<RawCodec, RawCodec> = Router::new();
        assert!(router.get_extractor().is_none());
    }
}
