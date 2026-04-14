#[cfg(test)]
mod tests {
    use aex::http::middlewares::logger::LogConfig;
    use std::sync::Arc;

    #[test]
    fn test_logger_new() {
        let logger = LogConfig::new();
        let executor = logger.build();
        assert!(Arc::strong_count(&executor) > 0);
    }

    #[test]
    fn test_logger_log_method() {
        let logger = LogConfig::new().log_method(true).build();
        assert!(Arc::strong_count(&logger) > 0);
    }

    #[test]
    fn test_logger_log_path() {
        let logger = LogConfig::new().log_path(true).build();
        assert!(Arc::strong_count(&logger) > 0);
    }

    #[test]
    fn test_logger_log_user_agent() {
        let logger = LogConfig::new().log_user_agent(true).build();
        assert!(Arc::strong_count(&logger) > 0);
    }

    #[test]
    fn test_logger_all() {
        let logger = LogConfig::new().all().build();
        assert!(Arc::strong_count(&logger) > 0);
    }

    #[test]
    fn test_logger_clone() {
        let logger1 = LogConfig::new().log_method(true);
        let logger2 = logger1.clone();
        let executor = logger2.build();
        assert!(Arc::strong_count(&executor) > 0);
    }
}
