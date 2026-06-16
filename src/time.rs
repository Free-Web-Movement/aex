use std::time::UNIX_EPOCH;

use chrono::{DateTime, Utc};

/// 全局 UTC 时间源
#[derive(Clone, Default)]
pub struct SystemTime;

impl SystemTime {
    /// 获取当前 UTC 时间
    pub fn now() -> DateTime<Utc> {
        Utc::now()
    }

    /// 当前秒级时间戳 (Unix Timestamp)
    pub fn now_ts() -> u64 {
        Utc::now().timestamp() as u64
    }

    pub fn timestamp() -> u128 {
        match std::time::SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(d) => d.as_millis(),
            Err(e) => {
                tracing::error!("SystemTime before UNIX_EPOCH: {}", e);
                0
            }
        }
    }

    /// 当前毫秒级时间戳
    pub fn now_ts_millis() -> u64 {
        Utc::now().timestamp_millis() as u64
    }

    /// 从时间戳恢复为 DateTime 对象
    pub fn from_timestamp(ts: u64) -> DateTime<Utc> {
        // 使用 i64 兼容 chrono 接口，0 为纳秒偏移
        DateTime::<Utc>::from_timestamp(ts as i64, 0).unwrap_or(Utc::now())
    }

    /// 校验给定的秒级时间戳是否在未来
    pub fn is_future(seconds: u64) -> bool {
        Self::now_ts() < seconds
    }

    /// 异步休眠（输入参数：秒）
    pub async fn sleep(seconds: u64) {
        // 🚀 修正：原代码中 seconds 传给 millis 会导致休眠时间缩短 1000 倍
        tokio::time::sleep(tokio::time::Duration::from_secs(seconds)).await;
    }

    /// 判断给定时间点是否已过期
    /// from: 起始时间, ttl_ms: 有效时长（毫秒）
    pub fn is_expired(from: DateTime<Utc>, ttl_ms: u128) -> bool {
        let now_ms = Utc::now().timestamp_millis();
        let from_ms = from.timestamp_millis();

        // 使用 saturating_sub 防止时间回拨导致的溢出 panic
        (now_ms.saturating_sub(from_ms)) as u128 >= ttl_ms
    }
}
