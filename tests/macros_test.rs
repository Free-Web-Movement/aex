#[cfg(test)]
mod tests {
    use aex::tcp::router::Router;

    #[test]
    fn test_macro_usable_via_on() {
        let router = Router::<(), ()>::new();
        assert_eq!(router.handlers.len(), 0);
    }
}
