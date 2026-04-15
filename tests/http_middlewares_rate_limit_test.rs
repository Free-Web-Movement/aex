use aex::http::middlewares::rate_limit::RateLimitConfig;

#[test]
fn test_rate_limit_config_new() {
    let config = RateLimitConfig::new(100, 60);
    let _executor = config.build();
}

#[test]
fn test_rate_limit_config_by_ip() {
    let config = RateLimitConfig::new(100, 60).by_ip();
    let _executor = config.build();
}

#[test]
fn test_rate_limit_config_by_header() {
    let config = RateLimitConfig::new(100, 60).by_header("X-API-Key");
    let _executor = config.build();
}

#[test]
fn test_rate_limit_config_by_path() {
    let config = RateLimitConfig::new(100, 60).by_path();
    let _executor = config.build();
}

#[test]
fn test_rate_limit_macro_default() {
    let _executor = aex::rate_limit!(100, 60);
}

#[test]
fn test_rate_limit_macro_by_ip() {
    let _executor = aex::rate_limit!(100, 60, by_ip);
}

#[test]
fn test_rate_limit_macro_by_header() {
    let _executor = aex::rate_limit!(100, 60, by_header: "X-API-Key");
}
