#[cfg(test)]
mod router_tests {
    use aex::tcp::router::Router;

    #[test]
    fn test_tcp_router_creation() {
        let router = Router::<(), ()>::new();
        assert_eq!(router.handlers.len(), 0);
    }
}
