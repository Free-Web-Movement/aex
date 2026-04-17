#[cfg(test)]
mod tests {
    use aex::tcp::router::Router;

    #[test]
    fn test_router_new() {
        let router = Router::<(), ()>::new();
        assert_eq!(router.handlers.len(), 0);
    }
}
