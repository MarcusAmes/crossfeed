use crate::RateLimiter;

#[tokio::test]
async fn rate_limiter_acquires() {
    let limiter = RateLimiter::new(1, 1);
    limiter.acquire().await;
    limiter.acquire().await;
}
