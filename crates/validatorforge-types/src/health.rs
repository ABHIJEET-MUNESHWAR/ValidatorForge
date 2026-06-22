//! Health snapshots reported by node agents and folded into ops decisions.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ids::Slot;

/// A coarse health classification derived from a [`HealthSnapshot`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    /// Caught up and voting within tolerances.
    Healthy,
    /// Falling behind but not yet delinquent (warning band).
    Lagging,
    /// Delinquent: slot lag or skip rate beyond the critical threshold.
    Delinquent,
    /// No fresh sample within the freshness window.
    Unknown,
}

impl HealthStatus {
    /// Stable wire string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            HealthStatus::Healthy => "healthy",
            HealthStatus::Lagging => "lagging",
            HealthStatus::Delinquent => "delinquent",
            HealthStatus::Unknown => "unknown",
        }
    }
}

impl std::fmt::Display for HealthStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A point-in-time health reading for one node.
///
/// `slot_lag` is how many slots behind the cluster tip the node is; `skip_rate`
/// is the fraction of recent leader slots skipped (0.0..=1.0); `cpu_load` is the
/// 1-minute load average normalised per core.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HealthSnapshot {
    tip_slot: Slot,
    node_slot: Slot,
    skip_rate: f64,
    cpu_load: f64,
    observed_at: DateTime<Utc>,
}

impl HealthSnapshot {
    /// Build a snapshot, clamping `skip_rate` and `cpu_load` to sane ranges.
    #[must_use]
    pub fn new(
        tip_slot: Slot,
        node_slot: Slot,
        skip_rate: f64,
        cpu_load: f64,
        observed_at: DateTime<Utc>,
    ) -> Self {
        Self {
            tip_slot,
            node_slot,
            skip_rate: skip_rate.clamp(0.0, 1.0),
            cpu_load: cpu_load.max(0.0),
            observed_at,
        }
    }

    /// Slots behind the cluster tip (saturating; never negative).
    #[must_use]
    pub fn slot_lag(&self) -> u64 {
        self.tip_slot.value().saturating_sub(self.node_slot.value())
    }

    /// Fraction of recent leader slots skipped.
    #[must_use]
    pub fn skip_rate(&self) -> f64 {
        self.skip_rate
    }

    /// Normalised CPU load.
    #[must_use]
    pub fn cpu_load(&self) -> f64 {
        self.cpu_load
    }

    /// When the snapshot was observed.
    #[must_use]
    pub fn observed_at(&self) -> DateTime<Utc> {
        self.observed_at
    }

    /// Classify health given the warning/critical thresholds.
    ///
    /// `lag_warn`/`lag_crit` are slot-lag bands; `skip_crit` is the critical
    /// skip-rate. A snapshot older than `freshness_secs` is [`HealthStatus::Unknown`].
    #[must_use]
    pub fn classify(
        &self,
        now: DateTime<Utc>,
        lag_warn: u64,
        lag_crit: u64,
        skip_crit: f64,
        freshness_secs: i64,
    ) -> HealthStatus {
        if (now - self.observed_at).num_seconds() > freshness_secs {
            return HealthStatus::Unknown;
        }
        let lag = self.slot_lag();
        if lag >= lag_crit || self.skip_rate >= skip_crit {
            HealthStatus::Delinquent
        } else if lag >= lag_warn {
            HealthStatus::Lagging
        } else {
            HealthStatus::Healthy
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_lag_saturates() {
        let s = HealthSnapshot::new(Slot(100), Slot(140), 0.0, 1.0, Utc::now());
        assert_eq!(s.slot_lag(), 0);
    }

    #[test]
    fn clamps_inputs() {
        let s = HealthSnapshot::new(Slot(100), Slot(90), 5.0, -3.0, Utc::now());
        assert_eq!(s.skip_rate(), 1.0);
        assert_eq!(s.cpu_load(), 0.0);
        assert_eq!(s.slot_lag(), 10);
    }

    #[test]
    fn classify_bands() {
        let now = Utc::now();
        let healthy = HealthSnapshot::new(Slot(1000), Slot(998), 0.01, 1.0, now);
        assert_eq!(
            healthy.classify(now, 32, 128, 0.5, 60),
            HealthStatus::Healthy
        );
        let lagging = HealthSnapshot::new(Slot(1000), Slot(960), 0.01, 1.0, now);
        assert_eq!(
            lagging.classify(now, 32, 128, 0.5, 60),
            HealthStatus::Lagging
        );
        let delinquent = HealthSnapshot::new(Slot(1000), Slot(800), 0.01, 1.0, now);
        assert_eq!(
            delinquent.classify(now, 32, 128, 0.5, 60),
            HealthStatus::Delinquent
        );
    }

    #[test]
    fn classify_skip_rate_critical() {
        let now = Utc::now();
        let s = HealthSnapshot::new(Slot(1000), Slot(999), 0.7, 1.0, now);
        assert_eq!(s.classify(now, 32, 128, 0.5, 60), HealthStatus::Delinquent);
    }

    #[test]
    fn stale_snapshot_is_unknown() {
        let observed = Utc::now() - chrono::Duration::seconds(120);
        let s = HealthSnapshot::new(Slot(1000), Slot(1000), 0.0, 1.0, observed);
        assert_eq!(
            s.classify(Utc::now(), 32, 128, 0.5, 60),
            HealthStatus::Unknown
        );
    }
}
