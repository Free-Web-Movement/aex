#[cfg(test)]
mod tests {
    use aex::tcp::router::UdpRouter;

    #[test]
    fn test_udp_router_creation() {
        let router = UdpRouter::new();
        assert_eq!(router.handlers.len(), 0);
    }
}
