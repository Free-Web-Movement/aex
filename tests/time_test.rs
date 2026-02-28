
#[cfg(test)]
mod tests {
    use aex::time::SystemTime;
    use chrono::Utc;

    #[test]
    fn test_now_consistency() {
        let ts_sec = SystemTime::now_ts();
        let ts_ms = SystemTime::now_ts_millis();
        
        // 验证秒级和毫秒级的一致性（允许 1 秒内的正常执行误差）
        assert!((ts_ms / 1000) >= ts_sec);
    }

    #[test]
    fn test_from_timestamp() {
        let original_ts = 1740700800; // 示例时间戳
        let dt = SystemTime::from_timestamp(original_ts);
        
        assert_eq!(dt.timestamp() as u64, original_ts);
        // 确保时区是 UTC
        assert_eq!(dt.timezone(), Utc);
    }

    #[test]
    fn test_is_future() {
        let past = SystemTime::now_ts() - 10;
        let future = SystemTime::now_ts() + 10;
        
        assert!(!SystemTime::is_future(past));
        assert!(SystemTime::is_future(future));
    }

    #[test]
    fn test_is_expired() {
        let now = SystemTime::now();
        
        // 1. 测试未过期：刚创建的时间，ttl 为 1000ms
        assert!(!SystemTime::is_expired(now, 1000));
        
        // 2. 测试已过期：500ms 前的时间，ttl 为 100ms
        let past_time = SystemTime::from_timestamp(SystemTime::now_ts() - 2);
        assert!(SystemTime::is_expired(past_time, 100));
    }

    #[tokio::test]
    async fn test_sleep_duration() {
        let start = SystemTime::now_ts_millis();
        let sleep_secs = 1;
        
        SystemTime::sleep(sleep_secs).await;
        
        let end = SystemTime::now_ts_millis();
        let elapsed = end - start;
        
        // 确保至少睡了 1000ms (允许调度误差)
        assert!(elapsed >= 1000);
        assert!(elapsed < 1100); // 正常情况下不应超过 1.1s
    }
}