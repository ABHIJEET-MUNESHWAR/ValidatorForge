//! Wall-clock adapter implementing the core [`Clock`] port.

use chrono::{DateTime, Utc};
use validatorforge_core::Clock;

/// A [`Clock`] backed by the system UTC wall clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct UtcClock;

impl UtcClock {
    /// Construct the clock.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Clock for UtcClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_is_monotonic_enough() {
        let c = UtcClock::new();
        let a = c.now();
        let b = c.now();
        assert!(b >= a);
    }
}
