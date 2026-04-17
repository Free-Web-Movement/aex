#[cfg(test)]
mod router_tests {
    use aex::tcp::router::TcpRouter;

    #[test]
    fn test_tcp_router_creation() {
        let router = TcpRouter::new();
        assert_eq!(router.handlers.len(), 0);
    }
}
