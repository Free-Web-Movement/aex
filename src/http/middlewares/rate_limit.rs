use ahash::AHashMap;
use parking_lot::RwLock;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::{
    connection::context::Context,
    exe,
    http::{meta::HttpMetadata, protocol::status::StatusCode, types::Executor, protocol::header::HeaderKey},
};

#[derive(Clone)]
pub struct RateLimitConfig {
    max_requests: usize,
    window_secs: u64,
    key_fn: Arc<dyn Fn(&Context) -> String + Send + Sync>,
}

impl RateLimitConfig {
    pub fn new(max_requests: usize, window_secs: u64) -> Self {
        Self {
            max_requests,
            window_secs,
            key_fn: Arc::new(|ctx| {
                ctx.addr.to_string()
            }),
        }
    }

    pub fn by_ip(mut self) -> Self {
        self.key_fn = Arc::new(|ctx| {
            ctx.addr.to_string()
        });
        self
    }

    pub fn by_header(mut self, header: &str) -> Self {
        let header_name = header.to_string();
        self.key_fn = Arc::new(move |ctx| {
            if let Some(meta) = ctx.local.get_ref::<HttpMetadata>() {
                meta.headers
                    .get(&HeaderKey::from(header_name.as_str()))
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_string())
            } else {
                "unknown".to_string()
            }
        });
        self
    }

    pub fn by_path(mut self) -> Self {
        self.key_fn = Arc::new(|ctx| {
            if let Some(meta) = ctx.local.get_ref::<HttpMetadata>() {
                format!("{}:{}", meta.path, ctx.addr)
            } else {
                ctx.addr.to_string()
            }
        });
        self
    }

    pub fn build(self) -> Arc<Executor> {
        let state = Arc::new(RwLock::new(AHashMap::<String, RateLimitBucket>::new()));
        let config = Arc::new(self);

        exe!(move |ctx, data| {
            let (config, key, state) = data;
            let now = Instant::now();
            let window = Duration::from_secs(config.window_secs);

            let mut state = state.write();
            let bucket = state.entry(key.clone()).or_insert_with(|| RateLimitBucket {
                tokens: config.max_requests,
                last_refill: now,
            });

            if now.duration_since(bucket.last_refill) >= window {
                bucket.tokens = config.max_requests;
                bucket.last_refill = now;
            }

            if bucket.tokens > 0 {
                bucket.tokens -= 1;
                let remaining = bucket.tokens;
                let reset = bucket.last_refill + window;

                ctx.res()
                    .set_header("X-RateLimit-Limit", config.max_requests.to_string())
                    .set_header("X-RateLimit-Remaining", remaining.to_string())
                    .set_header("X-RateLimit-Reset", reset.elapsed().as_secs().to_string());

                true
            } else {
                let retry_after = bucket.last_refill + window - now;
                let retry_after_secs = retry_after.as_secs().to_string();
                ctx.status(StatusCode::TooManyRequests).send(
                    format!(
                        "Rate limit exceeded. Retry after {} seconds.",
                        retry_after_secs
                    ),
                    None,
                );
                ctx.res()
                    .set_header("Retry-After", retry_after_secs);
                false
            }
        }, |ctx| {
            let config = config.clone();
            let key = (config.key_fn)(ctx);
            (config, key, state.clone())
        })
    }
}


struct RateLimitBucket {
    tokens: usize,
    last_refill: Instant,
}

#[macro_export]
macro_rules! rate_limit {
    ($max:expr, $window:expr) => {
        $crate::http::middlewares::rate_limit::RateLimitConfig::new($max, $window).build()
    };
    ($max:expr, $window:expr, by_ip) => {
        $crate::http::middlewares::rate_limit::RateLimitConfig::new($max, $window)
            .by_ip()
            .build()
    };
    ($max:expr, $window:expr, by_header: $header:expr) => {
        $crate::http::middlewares::rate_limit::RateLimitConfig::new($max, $window)
            .by_header($header)
            .build()
    };
}
