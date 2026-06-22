//! Token-bucket rate limiter generic over a [`Clock`].

use parking_lot::Mutex;

use crate::clock::{Clock, SystemClock};

#[derive(Debug)]
struct Bucket {
    tokens: f64,
    last_refill_millis: u64,
}

/// A token-bucket rate limiter. `capacity` is the burst size; `refill_per_sec`
/// is the steady-state admission rate.
#[derive(Debug)]
pub struct RateLimiter<C: Clock = SystemClock> {
    clock: C,
    capacity: f64,
    refill_per_sec: f64,
    bucket: Mutex<Bucket>,
}

impl<C: Clock> RateLimiter<C> {
    /// Create a limiter, pre-filled to `capacity`.
    #[must_use]
    pub fn new(clock: C, capacity: f64, refill_per_sec: f64) -> Self {
        let now = clock.now_millis();
        Self {
            clock,
            capacity: capacity.max(0.0),
            refill_per_sec: refill_per_sec.max(0.0),
            bucket: Mutex::new(Bucket {
                tokens: capacity.max(0.0),
                last_refill_millis: now,
            }),
        }
    }

    fn refill(&self, bucket: &mut Bucket) {
        let now = self.clock.now_millis();
        let elapsed_ms = now.saturating_sub(bucket.last_refill_millis);
        if elapsed_ms > 0 {
            let added = (elapsed_ms as f64 / 1000.0) * self.refill_per_sec;
            bucket.tokens = (bucket.tokens + added).min(self.capacity);
            bucket.last_refill_millis = now;
        }
    }

    /// Try to consume one token; returns `true` if admitted.
    pub fn try_acquire(&self) -> bool {
        self.try_acquire_n(1.0)
    }

    /// Try to consume `n` tokens; returns `true` if admitted.
    pub fn try_acquire_n(&self, n: f64) -> bool {
        let mut bucket = self.bucket.lock();
        self.refill(&mut bucket);
        if bucket.tokens >= n {
            bucket.tokens -= n;
            true
        } else {
            false
        }
    }

    /// Currently available tokens (after refill), for observability.
    pub fn available(&self) -> f64 {
        let mut bucket = self.bucket.lock();
        self.refill(&mut bucket);
        bucket.tokens
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::ManualClock;

    #[test]
    fn admits_up_to_capacity_then_throttles() {
        let clock = ManualClock::new(0);
        let rl = RateLimiter::new(clock, 3.0, 1.0);
        assert!(rl.try_acquire());
        assert!(rl.try_acquire());
        assert!(rl.try_acquire());
        assert!(!rl.try_acquire());
    }

    #[test]
    fn refills_over_time() {
        let clock = ManualClock::new(0);
        let rl = RateLimiter::new(clock.clone(), 2.0, 10.0);
        assert!(rl.try_acquire());
        assert!(rl.try_acquire());
        assert!(!rl.try_acquire());
        // 10 tokens/sec => 100ms yields 1 token.
        clock.advance(100);
        assert!(rl.try_acquire());
        assert!(!rl.try_acquire());
    }

    #[test]
    fn never_exceeds_capacity() {
        let clock = ManualClock::new(0);
        let rl = RateLimiter::new(clock.clone(), 5.0, 1000.0);
        clock.advance(10_000);
        assert!((rl.available() - 5.0).abs() < 1e-9);
    }

    #[test]
    fn acquire_n() {
        let clock = ManualClock::new(0);
        let rl = RateLimiter::new(clock, 10.0, 1.0);
        assert!(rl.try_acquire_n(7.0));
        assert!(!rl.try_acquire_n(7.0));
        assert!(rl.try_acquire_n(3.0));
    }
}
