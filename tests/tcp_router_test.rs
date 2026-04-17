#[cfg(test)]
mod tests {
    use aex::tcp::router::TcpRouter;

    #[test]
    fn test_router_new() {
        let router = TcpRouter::new();
        assert_eq!(router.handlers.len(), 0);
    }
}
