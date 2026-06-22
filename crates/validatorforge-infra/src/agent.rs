//! A simulated [`NodeAgent`] used by the standalone node binary and tests.
//!
//! It performs no real I/O: each operation succeeds after a tiny simulated
//! delay, records the action it was asked to perform, and (optionally) fails a
//! configured action so the saga's compensation path can be exercised
//! end-to-end without provisioning real hardware.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::Mutex;
use validatorforge_core::{NodeAgent, PortError};
use validatorforge_types::{HostAddr, OpsActionKind, Slot, ValidatorVersion};

/// In-memory node agent that simulates fleet operations.
#[derive(Clone)]
pub struct SimNodeAgent {
    tip: Arc<AtomicU64>,
    delay: Duration,
    fail_on: Arc<HashSet<OpsActionKind>>,
    log: Arc<Mutex<Vec<OpsActionKind>>>,
}

impl SimNodeAgent {
    /// A simulator that succeeds at every operation.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tip: Arc::new(AtomicU64::new(1_000)),
            delay: Duration::from_millis(1),
            fail_on: Arc::new(HashSet::new()),
            log: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// A simulator that fails (with a retryable error) on the given actions.
    #[must_use]
    pub fn failing_on(actions: impl IntoIterator<Item = OpsActionKind>) -> Self {
        let mut s = Self::new();
        s.fail_on = Arc::new(actions.into_iter().collect());
        s
    }

    /// The recorded sequence of actions performed.
    #[must_use]
    pub fn calls(&self) -> Vec<OpsActionKind> {
        self.log.lock().clone()
    }

    async fn run(&self, action: OpsActionKind) -> Result<(), PortError> {
        tokio::time::sleep(self.delay).await;
        self.log.lock().push(action);
        if self.fail_on.contains(&action) {
            return Err(PortError::Unavailable(format!(
                "simulated failure on {action}"
            )));
        }
        Ok(())
    }
}

impl Default for SimNodeAgent {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl NodeAgent for SimNodeAgent {
    async fn apply_infra(&self, _host: &HostAddr) -> Result<(), PortError> {
        self.run(OpsActionKind::ApplyInfra).await
    }

    async fn tune_host(&self, _host: &HostAddr) -> Result<(), PortError> {
        self.run(OpsActionKind::TuneHost).await
    }

    async fn start_validator(
        &self,
        _host: &HostAddr,
        _version: &ValidatorVersion,
    ) -> Result<(), PortError> {
        self.run(OpsActionKind::StartValidator).await
    }

    async fn await_catchup(&self, _host: &HostAddr) -> Result<Slot, PortError> {
        self.run(OpsActionKind::AwaitCatchup).await?;
        Ok(Slot(self.tip.fetch_add(1, Ordering::SeqCst)))
    }

    async fn drain(&self, _host: &HostAddr) -> Result<(), PortError> {
        self.run(OpsActionKind::Drain).await
    }

    async fn swap_identity(&self, _from: &HostAddr, _to: &HostAddr) -> Result<(), PortError> {
        self.run(OpsActionKind::SwapIdentity).await
    }

    async fn destroy_infra(&self, _host: &HostAddr) -> Result<(), PortError> {
        self.run(OpsActionKind::DestroyInfra).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host() -> HostAddr {
        HostAddr::new("sim.internal").unwrap()
    }

    #[tokio::test]
    async fn records_calls_in_order() {
        let agent = SimNodeAgent::new();
        agent.apply_infra(&host()).await.unwrap();
        agent.tune_host(&host()).await.unwrap();
        assert_eq!(
            agent.calls(),
            vec![OpsActionKind::ApplyInfra, OpsActionKind::TuneHost]
        );
    }

    #[tokio::test]
    async fn catchup_returns_increasing_slot() {
        let agent = SimNodeAgent::new();
        let a = agent.await_catchup(&host()).await.unwrap();
        let b = agent.await_catchup(&host()).await.unwrap();
        assert!(b.value() > a.value());
    }

    #[tokio::test]
    async fn injected_failure_is_returned() {
        let agent = SimNodeAgent::failing_on([OpsActionKind::StartValidator]);
        let v = ValidatorVersion::new("2.0.14").unwrap();
        let err = agent.start_validator(&host(), &v).await.unwrap_err();
        assert!(err.is_retryable());
    }
}
