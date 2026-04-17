#[cfg(test)]
mod tests {
    use aex::udp::router::Router;

    #[test]
    fn test_udp_router_new() {
        let router = Router::<(), ()>::new();
        assert_eq!(router.handlers.len(), 0);
    }
}
