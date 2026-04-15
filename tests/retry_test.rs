use aex::connection::retry::{RetryAction, RetryConfig, RetryManager};

#[test]
fn test_retry_config_new() {
    let config = RetryConfig::new(5);
    assert_eq!(config.max_retries, 5);
    assert_eq!(config.initial_delay_ms, 1000);
    assert_eq!(config.max_delay_ms, 30000);
    assert_eq!(config.backoff_factor, 2.0);
}

#[test]
fn test_retry_config_builder() {
    let config = RetryConfig::new(3)
        .with_initial_delay(500)
        .with_max_delay(10000)
        .with_backoff_factor(1.5);

    assert_eq!(config.initial_delay_ms, 500);
    assert_eq!(config.max_delay_ms, 10000);
    assert_eq!(config.backoff_factor, 1.5);
}

#[test]
fn test_retry_config_default() {
    let config = RetryConfig::default();
    assert_eq!(config.max_retries, 5);
}

#[test]
fn test_retry_config_calculate_delay() {
    let config = RetryConfig::new(5);

    let delay0 = config.calculate_delay(0);
    assert_eq!(delay0.as_millis(), 1000);

    let delay1 = config.calculate_delay(1);
    assert_eq!(delay1.as_millis(), 2000);

    let delay2 = config.calculate_delay(2);
    assert_eq!(delay2.as_millis(), 4000);
}

#[test]
fn test_retry_config_calculate_delay_max() {
    let config = RetryConfig::new(10).with_max_delay(1500);

    let delay = config.calculate_delay(10);
    assert_eq!(delay.as_millis(), 1500);
}

#[test]
fn test_retry_manager_new() {
    let config = RetryConfig::new(5);
    let manager = RetryManager::new(config);
    assert_eq!(manager.attempt(), 0);
}

#[test]
fn test_retry_manager_next_retry() {
    let config = RetryConfig::new(3);
    let mut manager = RetryManager::new(config);

    let action = manager.next();
    assert!(matches!(action, RetryAction::Retry(_)));
    assert_eq!(manager.attempt(), 1);
}

#[test]
fn test_retry_manager_next_stop() {
    let config = RetryConfig::new(1);
    let mut manager = RetryManager::new(config);

    manager.next();
    let action = manager.next();
    assert!(matches!(action, RetryAction::Stop));
}

#[test]
fn test_retry_manager_should_retry() {
    let config = RetryConfig::new(3);
    let mut manager = RetryManager::new(config);

    assert!(manager.should_retry());
    manager.next();
    assert!(manager.should_retry());
    manager.next();
    assert!(manager.should_retry());
    manager.next();
    assert!(!manager.should_retry());
}

#[test]
fn test_retry_manager_reset() {
    let config = RetryConfig::new(3);
    let mut manager = RetryManager::new(config);

    manager.next();
    manager.next();
    assert_eq!(manager.attempt(), 2);

    manager.reset();
    assert_eq!(manager.attempt(), 0);
}
