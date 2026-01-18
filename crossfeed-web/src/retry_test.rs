use crate::RetryPolicy;

#[test]
fn retry_policy_backoff_caps() {
    let policy = RetryPolicy::default();
    let delay = policy.next_delay(5);
    assert_eq!(delay, policy.max_delay);
}
