use std::time::Duration;

#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub max_retries: usize,
    pub base_delay: Duration,
    pub max_delay: Duration,
    pub retry_on_5xx: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 2,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_millis(300),
            retry_on_5xx: true,
        }
    }
}

#[derive(Debug, Clone)]
pub enum RetryableError {
    Network,
    Timeout,
    ServerError(u16),
}

impl RetryPolicy {
    pub fn next_delay(&self, attempt: usize) -> Duration {
        let multiplier = 1u64 << attempt.min(3);
        let delay = self.base_delay.saturating_mul(multiplier as u32);
        if delay > self.max_delay {
            self.max_delay
        } else {
            delay
        }
    }
}
