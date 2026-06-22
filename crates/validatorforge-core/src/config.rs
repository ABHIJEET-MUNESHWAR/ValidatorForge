//! Tunable configuration for the ops engine and its resilience envelope.

use std::time::Duration;

/// Health classification thresholds and resilience knobs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpsConfig {
    /// Slot lag at which a node is considered `Lagging`.
    pub lag_warn: u64,
    /// Slot lag at which a node is considered `Delinquent`.
    pub lag_crit: u64,
    /// Skip rate (0.0..=1.0) at which a node is considered `Delinquent`.
    pub skip_crit: f64,
    /// A health snapshot older than this many seconds is `Unknown`.
    pub freshness_secs: i64,
    /// Per-agent-call timeout.
    pub op_timeout: Duration,
    /// Total attempts per agent call (incl. first).
    pub max_attempts: u32,
    /// Base retry backoff.
    pub retry_base: Duration,
    /// Max retry backoff.
    pub retry_max: Duration,
    /// Consecutive failures before the breaker trips open.
    pub breaker_threshold: u32,
    /// Breaker open cooldown, in milliseconds.
    pub breaker_cooldown_ms: u64,
    /// Admission rate-limit burst capacity (deployments).
    pub rate_capacity: f64,
    /// Admission rate-limit steady rate (deployments/sec).
    pub rate_per_sec: f64,
    /// Max concurrent in-flight deployments (bulkhead).
    pub max_concurrent_deploys: usize,
}

impl Default for OpsConfig {
    fn default() -> Self {
        Self {
            lag_warn: 32,
            lag_crit: 128,
            skip_crit: 0.5,
            freshness_secs: 60,
            op_timeout: Duration::from_secs(30),
            max_attempts: 3,
            retry_base: Duration::from_millis(50),
            retry_max: Duration::from_secs(5),
            breaker_threshold: 5,
            breaker_cooldown_ms: 10_000,
            rate_capacity: 16.0,
            rate_per_sec: 4.0,
            max_concurrent_deploys: 8,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let c = OpsConfig::default();
        assert!(c.lag_warn < c.lag_crit);
        assert!(c.skip_crit > 0.0 && c.skip_crit <= 1.0);
        assert!(c.max_attempts >= 1);
        assert!(c.max_concurrent_deploys >= 1);
    }
}
