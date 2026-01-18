use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct RateLimiter {
    state: Arc<Mutex<State>>,
    capacity: u32,
    refill_per_sec: u32,
}

#[derive(Debug)]
struct State {
    tokens: u32,
    last_refill: Instant,
}

impl RateLimiter {
    pub fn new(capacity: u32, refill_per_sec: u32) -> Self {
        let now = Instant::now();
        Self {
            state: Arc::new(Mutex::new(State {
                tokens: capacity,
                last_refill: now,
            })),
            capacity,
            refill_per_sec,
        }
    }

    pub async fn acquire(&self) {
        loop {
            let mut state = self.state.lock().await;
            self.refill(&mut state);
            if state.tokens > 0 {
                state.tokens -= 1;
                return;
            }
            drop(state);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    fn refill(&self, state: &mut State) {
        let now = Instant::now();
        let elapsed = now.duration_since(state.last_refill);
        let refill = (elapsed.as_secs_f64() * self.refill_per_sec as f64) as u32;
        if refill > 0 {
            state.tokens = (state.tokens + refill).min(self.capacity);
            state.last_refill = now;
        }
    }
}
