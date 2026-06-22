//! [`OpsEngine`]: the application service that composes ports + resilience +
//! saga orchestration into the use-cases the API layer calls.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use validatorforge_resilience::{Bulkhead, RateLimiter, SystemClock};
use validatorforge_types::{
    DeploymentKind, DeploymentRun, HealthSnapshot, HealthStatus, NodeId, NodeState, RunId,
    RunStatus, ValidatorNode,
};

use crate::agent::ResilientNodeAgent;
use crate::config::OpsConfig;
use crate::error::CoreError;
use crate::event::OpsEvent;
use crate::ports::{Clock, EventSink, NodeAgent, NodeRepository, OpsAdvisor};
use crate::saga::{build_plan, SagaContext, SagaExecutor};

/// The orchestration service. Cheap to clone-wrap in an `Arc`.
pub struct OpsEngine {
    repo: Arc<dyn NodeRepository>,
    agent: Arc<dyn NodeAgent>,
    events: Arc<dyn EventSink>,
    advisor: Arc<dyn OpsAdvisor>,
    clock: Arc<dyn Clock>,
    saga: SagaExecutor,
    limiter: RateLimiter<SystemClock>,
    bulkhead: Bulkhead,
    cfg: OpsConfig,
    next_run: AtomicU64,
}

impl OpsEngine {
    /// Wire the engine. `raw_agent` is wrapped in a [`ResilientNodeAgent`] so all
    /// saga effects inherit timeout + retry + breaker.
    #[must_use]
    pub fn new(
        repo: Arc<dyn NodeRepository>,
        raw_agent: Arc<dyn NodeAgent>,
        events: Arc<dyn EventSink>,
        advisor: Arc<dyn OpsAdvisor>,
        clock: Arc<dyn Clock>,
        cfg: OpsConfig,
    ) -> Self {
        let agent: Arc<dyn NodeAgent> = Arc::new(ResilientNodeAgent::new(raw_agent, &cfg));
        let saga = SagaExecutor::new(events.clone(), clock.clone());
        let limiter = RateLimiter::new(SystemClock::new(), cfg.rate_capacity, cfg.rate_per_sec);
        let bulkhead = Bulkhead::new(cfg.max_concurrent_deploys);
        Self {
            repo,
            agent,
            events,
            advisor,
            clock,
            saga,
            limiter,
            bulkhead,
            cfg,
            next_run: AtomicU64::new(1),
        }
    }

    /// Register (upsert) a node in [`NodeState::Provisioning`].
    ///
    /// # Errors
    /// Propagates repository failures.
    pub async fn register_node(&self, node: ValidatorNode) -> Result<(), CoreError> {
        self.repo.save_node(&node).await?;
        Ok(())
    }

    /// Run a deployment saga end-to-end and return the terminal run record.
    ///
    /// Admission is gated by a token-bucket rate limiter and a bounded bulkhead;
    /// either exhausted yields [`CoreError::Throttled`].
    ///
    /// # Errors
    /// - [`CoreError::Throttled`] when over the rate/concurrency budget,
    /// - [`CoreError::NotFound`] when the node (or failover spare) is unknown,
    /// - [`CoreError::Domain`] / [`CoreError::Port`] on illegal transition / repo failure.
    pub async fn start_deployment(
        &self,
        node_id: &NodeId,
        kind: DeploymentKind,
    ) -> Result<DeploymentRun, CoreError> {
        if !self.limiter.try_acquire() {
            metrics::counter!("validatorforge_deploy_throttled_total").increment(1);
            return Err(CoreError::Throttled);
        }
        let _slot = self.bulkhead.try_acquire().ok_or(CoreError::Throttled)?;

        let mut node = self
            .repo
            .get_node(node_id)
            .await?
            .ok_or_else(|| CoreError::NotFound(node_id.to_string()))?;

        let spare_host = match &kind {
            DeploymentKind::Failover { spare } => {
                let sp = self
                    .repo
                    .get_node(spare)
                    .await?
                    .ok_or_else(|| CoreError::NotFound(spare.to_string()))?;
                Some(sp.host().clone())
            }
            _ => None,
        };
        let version = match &kind {
            DeploymentKind::Upgrade { target_version } => target_version.clone(),
            _ => node.version().clone(),
        };

        let run_id = RunId(u128::from(self.next_run.fetch_add(1, Ordering::SeqCst)));
        let mut run = DeploymentRun::new(run_id, node.id().clone(), kind.clone(), self.clock.now());
        self.repo.save_run(&run).await?;
        self.events
            .publish(OpsEvent::RunStarted {
                run: run_id,
                node: node.id().clone(),
                kind: kind.clone(),
                at: self.clock.now(),
            })
            .await;

        let ctx = SagaContext {
            agent: self.agent.clone(),
            host: node.host().clone(),
            version,
            spare_host,
        };
        let steps = build_plan(&kind);
        let status = self.saga.run(&steps, &ctx, &mut run).await;

        let path = if status == RunStatus::Succeeded {
            Self::success_path(&kind)
        } else {
            Self::failure_path(&kind)
        };
        for to in path {
            let from = node.state();
            node.transition_to(to, self.clock.now())?;
            self.events
                .publish(OpsEvent::NodeStateChanged {
                    node: node.id().clone(),
                    from,
                    to,
                    at: self.clock.now(),
                })
                .await;
        }
        if status == RunStatus::Succeeded {
            if let DeploymentKind::Upgrade { target_version } = &kind {
                node.set_version(target_version.clone(), self.clock.now());
            }
        }
        self.repo.save_node(&node).await?;

        run.finish(status, self.clock.now());
        self.repo.save_run(&run).await?;
        self.events
            .publish(OpsEvent::RunFinished {
                run: run_id,
                status,
                at: self.clock.now(),
            })
            .await;
        metrics::counter!("validatorforge_deploys_total", "status" => status.as_str()).increment(1);
        Ok(run)
    }

    /// Node-state transitions to apply after a successful saga of `kind`.
    fn success_path(kind: &DeploymentKind) -> Vec<NodeState> {
        match kind {
            DeploymentKind::Provision => vec![
                NodeState::Bootstrapping,
                NodeState::CatchingUp,
                NodeState::Active,
            ],
            DeploymentKind::Upgrade { .. } => vec![
                NodeState::Draining,
                NodeState::CatchingUp,
                NodeState::Active,
            ],
            DeploymentKind::Failover { .. } | DeploymentKind::Decommission => {
                vec![NodeState::Draining, NodeState::Decommissioned]
            }
        }
    }

    /// Node-state transitions to apply after a failed/rolled-back saga.
    fn failure_path(kind: &DeploymentKind) -> Vec<NodeState> {
        match kind {
            // A provisioning node that never came up is terminal-failed.
            DeploymentKind::Provision => vec![NodeState::Failed],
            // For the rest, compensation restored a safe state; leave as-is.
            _ => Vec::new(),
        }
    }

    /// Classify a health snapshot, emit an event, and return the status.
    ///
    /// # Errors
    /// [`CoreError::NotFound`] when the node is unknown; repo failures propagate.
    pub async fn evaluate_health(
        &self,
        node_id: &NodeId,
        snapshot: &HealthSnapshot,
    ) -> Result<HealthStatus, CoreError> {
        self.repo
            .get_node(node_id)
            .await?
            .ok_or_else(|| CoreError::NotFound(node_id.to_string()))?;
        let status = snapshot.classify(
            self.clock.now(),
            self.cfg.lag_warn,
            self.cfg.lag_crit,
            self.cfg.skip_crit,
            self.cfg.freshness_secs,
        );
        self.events
            .publish(OpsEvent::HealthEvaluated {
                node: node_id.clone(),
                status,
                at: self.clock.now(),
            })
            .await;
        Ok(status)
    }

    /// Ask the AI advisor for a recommendation about a node.
    ///
    /// # Errors
    /// [`CoreError::NotFound`] when the node is unknown; repo failures propagate.
    pub async fn advise(&self, node_id: &NodeId, context: &str) -> Result<String, CoreError> {
        let node = self
            .repo
            .get_node(node_id)
            .await?
            .ok_or_else(|| CoreError::NotFound(node_id.to_string()))?;
        Ok(self.advisor.advise(&node, context).await)
    }

    /// Fetch a node by id.
    ///
    /// # Errors
    /// Repository failures propagate.
    pub async fn get_node(&self, node_id: &NodeId) -> Result<Option<ValidatorNode>, CoreError> {
        Ok(self.repo.get_node(node_id).await?)
    }

    /// List all nodes.
    ///
    /// # Errors
    /// Repository failures propagate.
    pub async fn list_nodes(&self) -> Result<Vec<ValidatorNode>, CoreError> {
        Ok(self.repo.list_nodes().await?)
    }

    /// Fetch a run by id.
    ///
    /// # Errors
    /// Repository failures propagate.
    pub async fn get_run(&self, run_id: RunId) -> Result<Option<DeploymentRun>, CoreError> {
        Ok(self.repo.get_run(run_id).await?)
    }

    /// List recent runs, newest first.
    ///
    /// # Errors
    /// Repository failures propagate.
    pub async fn list_runs(&self, limit: usize) -> Result<Vec<DeploymentRun>, CoreError> {
        Ok(self.repo.list_runs(limit).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::PortError;
    use crate::ports::{
        MockClock, MockEventSink, MockNodeAgent, MockNodeRepository, MockOpsAdvisor,
    };
    use chrono::Utc;
    use validatorforge_types::{Cluster, ClusterName, HostAddr, NodeRole, Slot, ValidatorVersion};

    fn node() -> ValidatorNode {
        ValidatorNode::new(
            NodeId::new("eu-val-01").unwrap(),
            ClusterName::new("eu-fiber").unwrap(),
            Cluster::Testnet,
            HostAddr::new("val01.internal").unwrap(),
            NodeRole::Voting,
            ValidatorVersion::new("2.0.14").unwrap(),
            Utc::now(),
        )
    }

    fn clock() -> MockClock {
        let mut c = MockClock::new();
        c.expect_now().returning(Utc::now);
        c
    }

    fn events() -> MockEventSink {
        let mut e = MockEventSink::new();
        e.expect_publish().returning(|_| ());
        e
    }

    fn cfg() -> OpsConfig {
        OpsConfig {
            op_timeout: std::time::Duration::from_millis(50),
            retry_base: std::time::Duration::from_millis(1),
            retry_max: std::time::Duration::from_millis(2),
            ..OpsConfig::default()
        }
    }

    #[tokio::test]
    async fn provision_drives_node_to_active() {
        let mut repo = MockNodeRepository::new();
        repo.expect_get_node().returning(|_| Ok(Some(node())));
        repo.expect_save_run().returning(|_| Ok(()));
        repo.expect_save_node().returning(|_| Ok(()));
        let mut agent = MockNodeAgent::new();
        agent.expect_apply_infra().returning(|_| Ok(()));
        agent.expect_tune_host().returning(|_| Ok(()));
        agent.expect_start_validator().returning(|_, _| Ok(()));
        agent.expect_await_catchup().returning(|_| Ok(Slot(10)));

        let engine = OpsEngine::new(
            Arc::new(repo),
            Arc::new(agent),
            Arc::new(events()),
            Arc::new(MockOpsAdvisor::new()),
            Arc::new(clock()),
            cfg(),
        );
        let run = engine
            .start_deployment(
                &NodeId::new("eu-val-01").unwrap(),
                DeploymentKind::Provision,
            )
            .await
            .unwrap();
        assert_eq!(run.status(), RunStatus::Succeeded);
    }

    #[tokio::test]
    async fn provision_failure_marks_node_failed() {
        let mut repo = MockNodeRepository::new();
        repo.expect_get_node().returning(|_| Ok(Some(node())));
        repo.expect_save_run().returning(|_| Ok(()));
        let saved = std::sync::Arc::new(std::sync::Mutex::new(None));
        let s2 = saved.clone();
        repo.expect_save_node().returning(move |n| {
            *s2.lock().unwrap() = Some(n.state());
            Ok(())
        });
        let mut agent = MockNodeAgent::new();
        agent
            .expect_apply_infra()
            .returning(|_| Err(PortError::Rejected("no capacity".into())));
        agent.expect_destroy_infra().returning(|_| Ok(()));

        let engine = OpsEngine::new(
            Arc::new(repo),
            Arc::new(agent),
            Arc::new(events()),
            Arc::new(MockOpsAdvisor::new()),
            Arc::new(clock()),
            cfg(),
        );
        let run = engine
            .start_deployment(
                &NodeId::new("eu-val-01").unwrap(),
                DeploymentKind::Provision,
            )
            .await
            .unwrap();
        assert_eq!(run.status(), RunStatus::RolledBack);
        assert_eq!(*saved.lock().unwrap(), Some(NodeState::Failed));
    }

    #[tokio::test]
    async fn unknown_node_is_not_found() {
        let mut repo = MockNodeRepository::new();
        repo.expect_get_node().returning(|_| Ok(None));
        let engine = OpsEngine::new(
            Arc::new(repo),
            Arc::new(MockNodeAgent::new()),
            Arc::new(events()),
            Arc::new(MockOpsAdvisor::new()),
            Arc::new(clock()),
            cfg(),
        );
        let err = engine
            .start_deployment(&NodeId::new("ghost").unwrap(), DeploymentKind::Provision)
            .await
            .unwrap_err();
        assert!(matches!(err, CoreError::NotFound(_)));
    }

    #[tokio::test]
    async fn evaluate_health_classifies() {
        let mut repo = MockNodeRepository::new();
        repo.expect_get_node().returning(|_| Ok(Some(node())));
        let engine = OpsEngine::new(
            Arc::new(repo),
            Arc::new(MockNodeAgent::new()),
            Arc::new(events()),
            Arc::new(MockOpsAdvisor::new()),
            Arc::new(clock()),
            cfg(),
        );
        let snap = HealthSnapshot::new(Slot(1000), Slot(998), 0.0, 1.0, Utc::now());
        let status = engine
            .evaluate_health(&NodeId::new("eu-val-01").unwrap(), &snap)
            .await
            .unwrap();
        assert_eq!(status, HealthStatus::Healthy);
    }

    #[tokio::test]
    async fn advise_delegates_to_advisor() {
        let mut repo = MockNodeRepository::new();
        repo.expect_get_node().returning(|_| Ok(Some(node())));
        let mut advisor = MockOpsAdvisor::new();
        advisor
            .expect_advise()
            .returning(|_, _| "scale up".to_string());
        let engine = OpsEngine::new(
            Arc::new(repo),
            Arc::new(MockNodeAgent::new()),
            Arc::new(events()),
            Arc::new(advisor),
            Arc::new(clock()),
            cfg(),
        );
        let out = engine
            .advise(&NodeId::new("eu-val-01").unwrap(), "lagging")
            .await
            .unwrap();
        assert_eq!(out, "scale up");
    }
}
