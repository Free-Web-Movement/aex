#[cfg(test)]
mod tests {
    use aex::http::middlewares::cors::CorsConfig;
    use std::sync::Arc;

    #[test]
    fn test_cors_config_default() {
        let _config = CorsConfig::default();
    }

    #[test]
    fn test_cors_config_new() {
        let _config = CorsConfig::new();
    }

    #[test]
    fn test_cors_build() {
        let config = CorsConfig::new();
        let executor = config.build();
        assert!(Arc::strong_count(&executor) > 0);
    }

    #[test]
    fn test_cors_builder_full() {
        let config = CorsConfig::new()
            .allow_origin_all(false)
            .allow_credentials(false)
            .max_age(3600);

        let executor = config.build();
        assert!(Arc::strong_count(&executor) > 0);
    }

    #[test]
    fn test_cors_builder_methods() {
        let _config = CorsConfig::new().allow_methods(vec!["GET", "POST"]);
    }

    #[test]
    fn test_cors_builder_headers() {
        let _config = CorsConfig::new().allow_headers(vec!["Content-Type"]);
    }
}
