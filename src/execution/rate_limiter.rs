use governor::clock::DefaultClock;
use governor::state::{InMemoryState, NotKeyed};
use governor::{Quota, RateLimiter as GovRateLimiter};
use std::num::NonZeroU32;
use std::sync::Arc;

pub struct RateLimiter {
    inner: Arc<GovRateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
}

impl RateLimiter {
    pub fn new(requests_per_second: u32, burst_capacity: u32) -> Self {
        let rps = NonZeroU32::new(requests_per_second).expect("rps must be > 0");
        let burst = NonZeroU32::new(burst_capacity).expect("burst must be > 0");

        let quota = Quota::per_second(rps).allow_burst(burst);

        Self {
            inner: Arc::new(GovRateLimiter::direct(quota)),
        }
    }

    pub fn with_default_config() -> Self {
        Self::new(15, 3)
    }

    pub async fn acquire(&self) {
        self.inner.until_ready().await;
    }

    pub fn try_acquire(&self) -> bool {
        self.inner.check().is_ok()
    }

    pub fn is_ready(&self) -> bool {
        self.inner.check().is_ok()
    }
}

impl Clone for RateLimiter {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::timeout;

    #[tokio::test]
    async fn test_rate_limiter_creation() {
        let limiter = RateLimiter::new(15, 3);
        assert!(limiter.try_acquire());
    }

    #[tokio::test]
    async fn test_rate_limiter_burst() {
        let limiter = RateLimiter::new(15, 3);

        assert!(limiter.try_acquire());
        assert!(limiter.try_acquire());
        assert!(limiter.try_acquire());
    }

    #[tokio::test]
    async fn test_rate_limiter_exhaustion() {
        let limiter = RateLimiter::new(1, 1);

        assert!(limiter.try_acquire());
        assert!(!limiter.try_acquire());
    }

    #[tokio::test]
    async fn test_rate_limiter_async_acquire() {
        let limiter = RateLimiter::new(100, 1);

        let result = timeout(Duration::from_millis(100), limiter.acquire()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_rate_limiter_is_ready() {
        let limiter = RateLimiter::new(1, 1);

        assert!(limiter.is_ready());
        limiter.try_acquire();
        assert!(!limiter.is_ready());
    }

    #[tokio::test]
    async fn test_rate_limiter_clone_shares_state() {
        let limiter1 = RateLimiter::new(1, 1);
        let limiter2 = limiter1.clone();

        assert!(limiter1.try_acquire());
        assert!(!limiter2.try_acquire());
    }

    #[tokio::test]
    async fn test_default_config() {
        let limiter = RateLimiter::with_default_config();
        for _ in 0..3 {
            assert!(limiter.try_acquire());
        }
    }
}
