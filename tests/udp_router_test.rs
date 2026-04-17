#[cfg(test)]
mod tests {
    use aex::tcp::router::UdpRouter;

    #[test]
    fn test_udp_router_new() {
        let router = UdpRouter::new();
        assert_eq!(router.handlers.len(), 0);
    }
}
