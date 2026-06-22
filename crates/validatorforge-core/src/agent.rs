//! [`ResilientNodeAgent`]: a decorator that wraps any [`NodeAgent`] so every
//! outbound call is guarded by circuit-breaker + bounded retry + per-attempt
//! timeout. This is how ValidatorForge satisfies "resilience on every boundary"
//! without sprinkling the logic across each saga step.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use validatorforge_resilience::{with_timeout, CircuitBreaker, RetryPolicy, SystemClock};
use validatorforge_types::{HostAddr, Slot, ValidatorVersion};

use crate::config::OpsConfig;
use crate::error::PortError;
use crate::ports::NodeAgent;

/// Decorator adding resilience to an inner [`NodeAgent`].
pub struct ResilientNodeAgent {
    inner: Arc<dyn NodeAgent>,
    breaker: Arc<CircuitBreaker<SystemClock>>,
    retry: RetryPolicy,
    op_timeout: Duration,
}

impl ResilientNodeAgent {
    /// Wrap `inner`, deriving the resilience envelope from `cfg`.
    #[must_use]
    pub fn new(inner: Arc<dyn NodeAgent>, cfg: &OpsConfig) -> Self {
        let breaker = Arc::new(CircuitBreaker::new(
            SystemClock::new(),
            cfg.breaker_threshold,
            cfg.breaker_cooldown_ms,
        ));
        Self {
            inner,
            breaker,
            retry: RetryPolicy::new(cfg.max_attempts, cfg.retry_base, cfg.retry_max),
            op_timeout: cfg.op_timeout,
        }
    }

    /// Current breaker state name, for metrics/observability.
    #[must_use]
    pub fn breaker_state(&self) -> &'static str {
        self.breaker.state_name()
    }

    /// Run `f` under breaker + retry + timeout.
    async fn guard<T, Fut, F>(&self, label: &'static str, f: F) -> Result<T, PortError>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<T, PortError>>,
    {
        if self.breaker.acquire().is_err() {
            metrics::counter!("validatorforge_agent_circuit_open_total", "op" => label)
                .increment(1);
            return Err(PortError::Unavailable(format!("circuit open for {label}")));
        }
        self.retry
            .retry(
                || async {
                    metrics::counter!("validatorforge_agent_calls_total", "op" => label)
                        .increment(1);
                    match with_timeout(self.op_timeout, f()).await {
                        Ok(Ok(v)) => {
                            self.breaker.on_success();
                            Ok(v)
                        }
                        Ok(Err(e)) => {
                            self.breaker.on_failure();
                            metrics::counter!("validatorforge_agent_failures_total", "op" => label)
                                .increment(1);
                            Err(e)
                        }
                        Err(_) => {
                            self.breaker.on_failure();
                            metrics::counter!("validatorforge_agent_timeouts_total", "op" => label)
                                .increment(1);
                            Err(PortError::Timeout(label.to_string()))
                        }
                    }
                },
                PortError::is_retryable,
            )
            .await
    }
}

#[async_trait]
impl NodeAgent for ResilientNodeAgent {
    async fn apply_infra(&self, host: &HostAddr) -> Result<(), PortError> {
        self.guard("apply_infra", || self.inner.apply_infra(host))
            .await
    }

    async fn tune_host(&self, host: &HostAddr) -> Result<(), PortError> {
        self.guard("tune_host", || self.inner.tune_host(host)).await
    }

    async fn start_validator(
        &self,
        host: &HostAddr,
        version: &ValidatorVersion,
    ) -> Result<(), PortError> {
        self.guard("start_validator", || {
            self.inner.start_validator(host, version)
        })
        .await
    }

    async fn await_catchup(&self, host: &HostAddr) -> Result<Slot, PortError> {
        self.guard("await_catchup", || self.inner.await_catchup(host))
            .await
    }

    async fn drain(&self, host: &HostAddr) -> Result<(), PortError> {
        self.guard("drain", || self.inner.drain(host)).await
    }

    async fn swap_identity(&self, from: &HostAddr, to: &HostAddr) -> Result<(), PortError> {
        self.guard("swap_identity", || self.inner.swap_identity(from, to))
            .await
    }

    async fn destroy_infra(&self, host: &HostAddr) -> Result<(), PortError> {
        self.guard("destroy_infra", || self.inner.destroy_infra(host))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::MockNodeAgent;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn cfg() -> OpsConfig {
        OpsConfig {
            max_attempts: 3,
            retry_base: Duration::from_millis(1),
            retry_max: Duration::from_millis(2),
            op_timeout: Duration::from_millis(50),
            breaker_threshold: 5,
            ..OpsConfig::default()
        }
    }

    #[tokio::test(start_paused = true)]
    async fn retries_transient_then_succeeds() {
        let calls = Arc::new(AtomicU32::new(0));
        let c = calls.clone();
        let mut mock = MockNodeAgent::new();
        mock.expect_apply_infra().returning(move |_| {
            let n = c.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                Err(PortError::Unavailable("warming up".into()))
            } else {
                Ok(())
            }
        });
        let agent = ResilientNodeAgent::new(Arc::new(mock), &cfg());
        let host = HostAddr::new("h").unwrap();
        assert!(agent.apply_infra(&host).await.is_ok());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn does_not_retry_rejected() {
        let calls = Arc::new(AtomicU32::new(0));
        let c = calls.clone();
        let mut mock = MockNodeAgent::new();
        mock.expect_drain().returning(move |_| {
            c.fetch_add(1, Ordering::SeqCst);
            Err(PortError::Rejected("nope".into()))
        });
        let agent = ResilientNodeAgent::new(Arc::new(mock), &cfg());
        let host = HostAddr::new("h").unwrap();
        assert!(agent.drain(&host).await.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn opens_breaker_after_threshold() {
        let mut mock = MockNodeAgent::new();
        mock.expect_tune_host()
            .returning(|_| Err(PortError::Unavailable("down".into())));
        let small = OpsConfig {
            breaker_threshold: 2,
            max_attempts: 1,
            ..cfg()
        };
        let agent = ResilientNodeAgent::new(Arc::new(mock), &small);
        let host = HostAddr::new("h").unwrap();
        let _ = agent.tune_host(&host).await;
        let _ = agent.tune_host(&host).await;
        // Breaker should now be open; next call fails fast as Unavailable.
        let err = agent.tune_host(&host).await.unwrap_err();
        assert!(matches!(err, PortError::Unavailable(_)));
        assert_eq!(agent.breaker_state(), "open");
    }
}
