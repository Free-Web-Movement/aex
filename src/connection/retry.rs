use std::time::Duration;

use tokio::sync::mpsc;

pub struct RetryConfig {
    pub max_retries: u32,
    pub initial_delay_ms: u64,
    pub max_delay_ms: u64,
    pub backoff_factor: f64,
}

impl RetryConfig {
    pub fn new(max_retries: u32) -> Self {
        Self {
            max_retries,
            initial_delay_ms: 1000,
            max_delay_ms: 30000,
            backoff_factor: 2.0,
        }
    }

    pub fn with_initial_delay(mut self, ms: u64) -> Self {
        self.initial_delay_ms = ms;
        self
    }

    pub fn with_max_delay(mut self, ms: u64) -> Self {
        self.max_delay_ms = ms;
        self
    }

    pub fn with_backoff_factor(mut self, factor: f64) -> Self {
        self.backoff_factor = factor;
        self
    }

    pub fn calculate_delay(&self, attempt: u32) -> Duration {
        let delay = (self.initial_delay_ms as f64 * self.backoff_factor.powi(attempt as i32)) as u64;
        let delay = delay.min(self.max_delay_ms);
        Duration::from_millis(delay)
    }
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self::new(5)
    }
}

pub enum RetryAction {
    Retry(Duration),
    Stop,
}

pub struct RetryManager {
    config: RetryConfig,
    attempt: u32,
}

impl RetryManager {
    pub fn new(config: RetryConfig) -> Self {
        Self { config, attempt: 0 }
    }

    pub fn next(&mut self) -> RetryAction {
        if self.attempt >= self.config.max_retries {
            return RetryAction::Stop;
        }
        let delay = self.config.calculate_delay(self.attempt);
        self.attempt += 1;
        RetryAction::Retry(delay)
    }

    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    pub fn reset(&mut self) {
        self.attempt = 0;
    }

    pub fn should_retry(&self) -> bool {
        self.attempt < self.config.max_retries
    }
}

pub async fn with_retry<F, T, E>(
    config: RetryConfig,
    mut operation: F,
) -> Result<T, E>
where
    F: FnMut() -> FutResult<T, E>,
    FutResult<T, E>: std::future::Future<Output = Result<T, E>>,
{
    let mut manager = RetryManager::new(config);
    loop {
        match manager.next() {
            RetryAction::Retry(delay) => {
                tokio::time::sleep(delay).await;
                match operation().await {
                    Ok(result) => return Ok(result),
                    Err(_) => continue,
                }
            }
            RetryAction::Stop => return operation().await,
        }
    }
}

pub type FutResult<T, E> = std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, E> + Send>>;