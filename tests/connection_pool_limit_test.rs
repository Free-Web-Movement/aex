#[cfg(test)]
mod tests {
    use aex::connection::pool_limit::ConnectionPoolConfig;

    #[test]
    fn test_pool_config_default() {
        let config = ConnectionPoolConfig::default();
        assert!(config.max_total_connections > 0);
    }

    #[test]
    fn test_pool_config_new() {
        let config = ConnectionPoolConfig::new(100);
        assert_eq!(config.max_total_connections, 100);
    }

    #[test]
    fn test_pool_config_with_per_ip_limit() {
        let config = ConnectionPoolConfig::new(100).with_per_ip_limit(5);
        assert_eq!(config.max_connections_per_ip, 5);
    }

    #[test]
    fn test_pool_config_with_subnet_limit() {
        let config = ConnectionPoolConfig::new(100).with_subnet_limit(50);
        assert_eq!(config.max_connections_per_subnet, 50);
    }

    #[test]
    fn test_pool_config_with_idle_timeout() {
        let config = ConnectionPoolConfig::new(100).with_idle_timeout(600);
        assert_eq!(config.idle_timeout_secs, 600);
    }
}
