//! Injectable monotonic clock abstraction.
//!
//! Timing-sensitive primitives (breaker cooldown, token-bucket refill) depend on
//! this trait rather than calling [`std::time::Instant`] directly, so unit tests
//! can drive time forward by hand with [`ManualClock`].

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// A source of monotonic milliseconds. Values are only meaningful relative to
/// each other (deltas), never as wall-clock time.
pub trait Clock: Send + Sync {
    /// Monotonic milliseconds since an arbitrary, fixed epoch.
    fn now_millis(&self) -> u64;
}

/// Real monotonic clock backed by [`Instant`].
#[derive(Debug, Clone)]
pub struct SystemClock {
    base: Instant,
}

impl SystemClock {
    /// Create a clock whose epoch is "now".
    #[must_use]
    pub fn new() -> Self {
        Self {
            base: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn now_millis(&self) -> u64 {
        // Saturating cast is fine: u64 millis covers ~584 million years.
        self.base.elapsed().as_millis() as u64
    }
}

/// A hand-driven clock for tests. Cloned handles share the same underlying
/// counter, so a clock injected into a primitive can still be advanced from the
/// test body.
#[derive(Debug, Clone, Default)]
pub struct ManualClock {
    millis: Arc<AtomicU64>,
}

impl ManualClock {
    /// Create a manual clock starting at `start_millis`.
    #[must_use]
    pub fn new(start_millis: u64) -> Self {
        Self {
            millis: Arc::new(AtomicU64::new(start_millis)),
        }
    }

    /// Advance the clock by `delta_millis`.
    pub fn advance(&self, delta_millis: u64) {
        self.millis.fetch_add(delta_millis, Ordering::SeqCst);
    }

    /// Set the clock to an absolute value.
    pub fn set(&self, millis: u64) {
        self.millis.store(millis, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now_millis(&self) -> u64 {
        self.millis.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_clock_advances() {
        let c = ManualClock::new(100);
        assert_eq!(c.now_millis(), 100);
        c.advance(50);
        assert_eq!(c.now_millis(), 150);
        c.set(0);
        assert_eq!(c.now_millis(), 0);
    }

    #[test]
    fn manual_clock_clones_share_state() {
        let a = ManualClock::new(0);
        let b = a.clone();
        a.advance(10);
        assert_eq!(b.now_millis(), 10);
    }

    #[test]
    fn system_clock_is_monotonic() {
        let c = SystemClock::new();
        let t0 = c.now_millis();
        let t1 = c.now_millis();
        assert!(t1 >= t0);
    }
}
