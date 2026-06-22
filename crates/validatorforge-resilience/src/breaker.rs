//! Circuit breaker generic over a [`Clock`].
//!
//! Trips `Open` after `failure_threshold` consecutive failures, stays open for
//! `cooldown`, then allows a single `HalfOpen` probe whose result decides
//! whether to close again or re-open.

use parking_lot::Mutex;
use thiserror::Error;

use crate::clock::{Clock, SystemClock};

/// Public view of the breaker's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakerState {
    /// Calls pass through; failures are counted.
    Closed,
    /// Calls are rejected until the cooldown elapses.
    Open,
    /// A single probe call is permitted.
    HalfOpen,
}

impl BreakerState {
    /// Stable wire string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            BreakerState::Closed => "closed",
            BreakerState::Open => "open",
            BreakerState::HalfOpen => "half_open",
        }
    }
}

/// Returned when the breaker rejects a call because it is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("circuit breaker is open")]
pub struct BreakerError;

#[derive(Debug)]
struct Inner {
    state: BreakerState,
    consecutive_failures: u32,
    opened_at_millis: u64,
}

/// A circuit breaker. Cheap to clone is *not* assumed; wrap in `Arc` to share.
#[derive(Debug)]
pub struct CircuitBreaker<C: Clock = SystemClock> {
    clock: C,
    failure_threshold: u32,
    cooldown_millis: u64,
    inner: Mutex<Inner>,
}

impl<C: Clock> CircuitBreaker<C> {
    /// Create a breaker with the given clock, failure threshold and cooldown.
    #[must_use]
    pub fn new(clock: C, failure_threshold: u32, cooldown_millis: u64) -> Self {
        Self {
            clock,
            failure_threshold: failure_threshold.max(1),
            cooldown_millis,
            inner: Mutex::new(Inner {
                state: BreakerState::Closed,
                consecutive_failures: 0,
                opened_at_millis: 0,
            }),
        }
    }

    /// Current state, applying any pending `Open -> HalfOpen` transition.
    pub fn state(&self) -> BreakerState {
        let mut inner = self.inner.lock();
        self.refresh(&mut inner);
        inner.state
    }

    /// Stable string for metrics/labels.
    pub fn state_name(&self) -> &'static str {
        self.state().as_str()
    }

    fn refresh(&self, inner: &mut Inner) {
        if inner.state == BreakerState::Open {
            let now = self.clock.now_millis();
            if now.saturating_sub(inner.opened_at_millis) >= self.cooldown_millis {
                inner.state = BreakerState::HalfOpen;
            }
        }
    }

    /// Ask permission to make a call.
    ///
    /// # Errors
    /// Returns [`BreakerError`] if the breaker is open.
    pub fn acquire(&self) -> Result<(), BreakerError> {
        let mut inner = self.inner.lock();
        self.refresh(&mut inner);
        match inner.state {
            BreakerState::Open => Err(BreakerError),
            BreakerState::Closed | BreakerState::HalfOpen => Ok(()),
        }
    }

    /// Record a successful call.
    pub fn on_success(&self) {
        let mut inner = self.inner.lock();
        inner.consecutive_failures = 0;
        inner.state = BreakerState::Closed;
    }

    /// Record a failed call, possibly tripping the breaker open.
    pub fn on_failure(&self) {
        let mut inner = self.inner.lock();
        match inner.state {
            BreakerState::HalfOpen => {
                inner.state = BreakerState::Open;
                inner.opened_at_millis = self.clock.now_millis();
                inner.consecutive_failures = self.failure_threshold;
            }
            _ => {
                inner.consecutive_failures += 1;
                if inner.consecutive_failures >= self.failure_threshold {
                    inner.state = BreakerState::Open;
                    inner.opened_at_millis = self.clock.now_millis();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::ManualClock;

    fn breaker(clock: ManualClock) -> CircuitBreaker<ManualClock> {
        CircuitBreaker::new(clock, 3, 1000)
    }

    #[test]
    fn opens_after_threshold() {
        let clock = ManualClock::new(0);
        let b = breaker(clock);
        assert!(b.acquire().is_ok());
        b.on_failure();
        b.on_failure();
        assert_eq!(b.state(), BreakerState::Closed);
        b.on_failure();
        assert_eq!(b.state(), BreakerState::Open);
        assert!(b.acquire().is_err());
    }

    #[test]
    fn half_open_after_cooldown_then_close_on_success() {
        let clock = ManualClock::new(0);
        let b = breaker(clock.clone());
        for _ in 0..3 {
            b.on_failure();
        }
        assert_eq!(b.state(), BreakerState::Open);
        clock.advance(1000);
        assert_eq!(b.state(), BreakerState::HalfOpen);
        assert!(b.acquire().is_ok());
        b.on_success();
        assert_eq!(b.state(), BreakerState::Closed);
    }

    #[test]
    fn half_open_failure_reopens() {
        let clock = ManualClock::new(0);
        let b = breaker(clock.clone());
        for _ in 0..3 {
            b.on_failure();
        }
        clock.advance(1000);
        assert_eq!(b.state(), BreakerState::HalfOpen);
        b.on_failure();
        assert_eq!(b.state(), BreakerState::Open);
    }

    #[test]
    fn success_resets_failure_count() {
        let clock = ManualClock::new(0);
        let b = breaker(clock);
        b.on_failure();
        b.on_failure();
        b.on_success();
        b.on_failure();
        b.on_failure();
        assert_eq!(b.state(), BreakerState::Closed);
    }

    #[test]
    fn state_names() {
        assert_eq!(BreakerState::HalfOpen.as_str(), "half_open");
    }
}
