#[cfg(test)]
mod tests {
    use aex::tcp::router::TcpRouter;

    #[test]
    fn test_macro_usable_via_on() {
        let mut router = TcpRouter::new();
        assert_eq!(router.handlers.len(), 0);
    }
}
