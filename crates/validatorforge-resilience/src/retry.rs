//! Bounded retry with equal-jitter exponential backoff.
//!
//! Jitter is derived from the process clock (`SystemTime` nanos) rather than a
//! `rand` dependency, keeping the crate dependency-light while still spreading
//! retries to avoid thundering herds.

use std::future::Future;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Configuration for a retry loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Total attempts including the first (so `3` means 1 try + 2 retries).
    pub max_attempts: u32,
    /// Base backoff before the first retry.
    pub base_delay: Duration,
    /// Upper bound on any single backoff.
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(50),
            max_delay: Duration::from_secs(5),
        }
    }
}

impl RetryPolicy {
    /// Construct a policy.
    #[must_use]
    pub fn new(max_attempts: u32, base_delay: Duration, max_delay: Duration) -> Self {
        Self {
            max_attempts: max_attempts.max(1),
            base_delay,
            max_delay,
        }
    }

    /// Equal-jitter backoff for the given zero-based retry index.
    ///
    /// `delay = backoff/2 + rand(0, backoff/2)` where `backoff = base * 2^n`,
    /// capped at `max_delay`.
    #[must_use]
    pub fn backoff_for(&self, retry_index: u32) -> Duration {
        let factor = 1u64.checked_shl(retry_index).unwrap_or(u64::MAX);
        let raw = self
            .base_delay
            .saturating_mul(u32::try_from(factor).unwrap_or(u32::MAX));
        let capped = raw.min(self.max_delay);
        let half = capped / 2;
        let jitter_span = capped.saturating_sub(half);
        let jitter = if jitter_span.is_zero() {
            Duration::ZERO
        } else {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0);
            let span = jitter_span.as_nanos().max(1) as u64;
            Duration::from_nanos(u64::from(nanos) % span)
        };
        half + jitter
    }

    /// Run `op` until it succeeds, the attempt budget is exhausted, or the error
    /// is classified non-retryable by `is_retryable`.
    ///
    /// # Errors
    /// Returns the last error produced by `op`.
    pub async fn retry<T, E, Fut, F, P>(&self, mut op: F, is_retryable: P) -> Result<T, E>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, E>>,
        P: Fn(&E) -> bool,
    {
        let mut attempt = 0u32;
        loop {
            match op().await {
                Ok(v) => return Ok(v),
                Err(e) => {
                    attempt += 1;
                    if attempt >= self.max_attempts || !is_retryable(&e) {
                        return Err(e);
                    }
                    let delay = self.backoff_for(attempt - 1);
                    if !delay.is_zero() {
                        tokio::time::sleep(delay).await;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    #[tokio::test(start_paused = true)]
    async fn succeeds_first_try() {
        let p = RetryPolicy::default();
        let calls = Arc::new(AtomicU32::new(0));
        let c = calls.clone();
        let r: Result<u32, ()> = p
            .retry(
                || {
                    let c = c.clone();
                    async move {
                        c.fetch_add(1, Ordering::SeqCst);
                        Ok(1)
                    }
                },
                |_| true,
            )
            .await;
        assert_eq!(r, Ok(1));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn retries_until_success() {
        let p = RetryPolicy::new(5, Duration::from_millis(1), Duration::from_millis(4));
        let calls = Arc::new(AtomicU32::new(0));
        let c = calls.clone();
        let r: Result<u32, &str> = p
            .retry(
                || {
                    let c = c.clone();
                    async move {
                        let n = c.fetch_add(1, Ordering::SeqCst);
                        if n >= 2 {
                            Ok(n)
                        } else {
                            Err("transient")
                        }
                    }
                },
                |_| true,
            )
            .await;
        assert_eq!(r, Ok(2));
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn stops_on_non_retryable() {
        let p = RetryPolicy::default();
        let calls = Arc::new(AtomicU32::new(0));
        let c = calls.clone();
        let r: Result<u32, &str> = p
            .retry(
                || {
                    let c = c.clone();
                    async move {
                        c.fetch_add(1, Ordering::SeqCst);
                        Err("fatal")
                    }
                },
                |_| false,
            )
            .await;
        assert_eq!(r, Err("fatal"));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn exhausts_attempts() {
        let p = RetryPolicy::new(3, Duration::from_millis(1), Duration::from_millis(2));
        let calls = Arc::new(AtomicU32::new(0));
        let c = calls.clone();
        let r: Result<u32, &str> = p
            .retry(
                || {
                    let c = c.clone();
                    async move {
                        c.fetch_add(1, Ordering::SeqCst);
                        Err("always")
                    }
                },
                |_| true,
            )
            .await;
        assert_eq!(r, Err("always"));
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn backoff_is_capped() {
        let p = RetryPolicy::new(10, Duration::from_millis(100), Duration::from_millis(400));
        for i in 0..10 {
            assert!(p.backoff_for(i) <= Duration::from_millis(400));
        }
    }
}
