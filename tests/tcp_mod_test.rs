#[cfg(test)]
mod tests {
    #[test]
    fn test_tcp_router_creation() {
        use aex::tcp::router::TcpRouter;

        let router = TcpRouter::new();
        assert_eq!(router.handlers.len(), 0);
    }
}
