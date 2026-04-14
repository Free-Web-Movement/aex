#[cfg(test)]
mod tests {
    #[test]
    fn test_tcp_router_creation() {
        let router = aex::tcp::router::Router::new();
        assert!(router.handlers.is_empty());
    }
}
