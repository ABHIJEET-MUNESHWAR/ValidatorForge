//! Compensating Saga orchestration.
//!
//! A deployment is modelled as an ordered list of [`SagaStep`]s. The
//! [`SagaExecutor`] runs them forward; if any step fails, it compensates the
//! already-completed steps in reverse order, leaving the fleet in a safe state.
//! [`build_plan`] maps a [`DeploymentKind`] to its concrete step list.

use std::sync::Arc;

use async_trait::async_trait;
use validatorforge_types::{
    DeploymentKind, DeploymentRun, HostAddr, OpsActionKind, RunStatus, ValidatorVersion,
};

use crate::error::PortError;
use crate::event::OpsEvent;
use crate::ports::{Clock, EventSink, NodeAgent};

/// Shared, read-only context handed to every step.
pub struct SagaContext {
    /// The (resilient) node agent used to perform effects.
    pub agent: Arc<dyn NodeAgent>,
    /// The primary host the saga targets.
    pub host: HostAddr,
    /// The validator version relevant to the saga (current or target).
    pub version: ValidatorVersion,
    /// The hot-spare host, when the saga is a failover.
    pub spare_host: Option<HostAddr>,
}

/// One reversible unit of work in a saga.
#[async_trait]
pub trait SagaStep: Send + Sync {
    /// The action this step represents (for the audit trail / events).
    fn action(&self) -> OpsActionKind;

    /// Perform the forward action.
    async fn execute(&self, ctx: &SagaContext) -> Result<(), PortError>;

    /// Undo the forward action. Defaults to a no-op for naturally idempotent
    /// or non-reversible-but-safe steps.
    async fn compensate(&self, _ctx: &SagaContext) -> Result<(), PortError> {
        Ok(())
    }
}

struct ApplyInfraStep;
#[async_trait]
impl SagaStep for ApplyInfraStep {
    fn action(&self) -> OpsActionKind {
        OpsActionKind::ApplyInfra
    }
    async fn execute(&self, ctx: &SagaContext) -> Result<(), PortError> {
        ctx.agent.apply_infra(&ctx.host).await
    }
    async fn compensate(&self, ctx: &SagaContext) -> Result<(), PortError> {
        ctx.agent.destroy_infra(&ctx.host).await
    }
}

struct TuneHostStep;
#[async_trait]
impl SagaStep for TuneHostStep {
    fn action(&self) -> OpsActionKind {
        OpsActionKind::TuneHost
    }
    async fn execute(&self, ctx: &SagaContext) -> Result<(), PortError> {
        ctx.agent.tune_host(&ctx.host).await
    }
}

struct StartValidatorStep;
#[async_trait]
impl SagaStep for StartValidatorStep {
    fn action(&self) -> OpsActionKind {
        OpsActionKind::StartValidator
    }
    async fn execute(&self, ctx: &SagaContext) -> Result<(), PortError> {
        ctx.agent.start_validator(&ctx.host, &ctx.version).await
    }
    async fn compensate(&self, ctx: &SagaContext) -> Result<(), PortError> {
        ctx.agent.drain(&ctx.host).await
    }
}

struct AwaitCatchupStep;
#[async_trait]
impl SagaStep for AwaitCatchupStep {
    fn action(&self) -> OpsActionKind {
        OpsActionKind::AwaitCatchup
    }
    async fn execute(&self, ctx: &SagaContext) -> Result<(), PortError> {
        ctx.agent.await_catchup(&ctx.host).await.map(|_| ())
    }
}

struct DrainStep;
#[async_trait]
impl SagaStep for DrainStep {
    fn action(&self) -> OpsActionKind {
        OpsActionKind::Drain
    }
    async fn execute(&self, ctx: &SagaContext) -> Result<(), PortError> {
        ctx.agent.drain(&ctx.host).await
    }
}

struct SwapIdentityStep;
#[async_trait]
impl SagaStep for SwapIdentityStep {
    fn action(&self) -> OpsActionKind {
        OpsActionKind::SwapIdentity
    }
    async fn execute(&self, ctx: &SagaContext) -> Result<(), PortError> {
        let spare = ctx
            .spare_host
            .as_ref()
            .ok_or_else(|| PortError::Rejected("failover requires a spare host".into()))?;
        ctx.agent.swap_identity(&ctx.host, spare).await
    }
    async fn compensate(&self, ctx: &SagaContext) -> Result<(), PortError> {
        // Swap the identity back onto the original host.
        if let Some(spare) = ctx.spare_host.as_ref() {
            ctx.agent.swap_identity(spare, &ctx.host).await
        } else {
            Ok(())
        }
    }
}

struct DestroyInfraStep;
#[async_trait]
impl SagaStep for DestroyInfraStep {
    fn action(&self) -> OpsActionKind {
        OpsActionKind::DestroyInfra
    }
    async fn execute(&self, ctx: &SagaContext) -> Result<(), PortError> {
        ctx.agent.destroy_infra(&ctx.host).await
    }
}

/// Build the ordered step list for a deployment kind.
#[must_use]
pub fn build_plan(kind: &DeploymentKind) -> Vec<Box<dyn SagaStep>> {
    match kind {
        DeploymentKind::Provision => vec![
            Box::new(ApplyInfraStep),
            Box::new(TuneHostStep),
            Box::new(StartValidatorStep),
            Box::new(AwaitCatchupStep),
        ],
        DeploymentKind::Upgrade { .. } => vec![
            Box::new(DrainStep),
            Box::new(StartValidatorStep),
            Box::new(AwaitCatchupStep),
        ],
        DeploymentKind::Failover { .. } => {
            vec![Box::new(DrainStep), Box::new(SwapIdentityStep)]
        }
        DeploymentKind::Decommission => vec![Box::new(DrainStep), Box::new(DestroyInfraStep)],
    }
}

/// Runs saga steps and compensates on failure, emitting events as it goes.
pub struct SagaExecutor {
    events: Arc<dyn EventSink>,
    clock: Arc<dyn Clock>,
}

impl SagaExecutor {
    /// Create an executor that publishes to `events` and timestamps via `clock`.
    #[must_use]
    pub fn new(events: Arc<dyn EventSink>, clock: Arc<dyn Clock>) -> Self {
        Self { events, clock }
    }

    /// Execute `steps` against `ctx`, recording progress into `run`.
    ///
    /// Returns the terminal [`RunStatus`]:
    /// - `Succeeded` if all steps completed,
    /// - `RolledBack` if a step failed but every compensation succeeded,
    /// - `Failed` if a step failed *and* a compensation also failed.
    pub async fn run(
        &self,
        steps: &[Box<dyn SagaStep>],
        ctx: &SagaContext,
        run: &mut DeploymentRun,
    ) -> RunStatus {
        let mut completed: Vec<&Box<dyn SagaStep>> = Vec::with_capacity(steps.len());
        for step in steps {
            match step.execute(ctx).await {
                Ok(()) => {
                    run.record_step(step.action());
                    self.events
                        .publish(OpsEvent::StepCompleted {
                            run: run.id(),
                            action: step.action(),
                            at: self.clock.now(),
                        })
                        .await;
                    completed.push(step);
                }
                Err(_) => {
                    metrics::counter!("validatorforge_saga_failures_total").increment(1);
                    let mut all_compensated = true;
                    for done in completed.iter().rev() {
                        if done.compensate(ctx).await.is_ok() {
                            self.events
                                .publish(OpsEvent::StepCompensated {
                                    run: run.id(),
                                    action: done.action(),
                                    at: self.clock.now(),
                                })
                                .await;
                        } else {
                            all_compensated = false;
                        }
                    }
                    return if all_compensated {
                        RunStatus::RolledBack
                    } else {
                        RunStatus::Failed
                    };
                }
            }
        }
        RunStatus::Succeeded
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::{MockClock, MockEventSink, MockNodeAgent};
    use chrono::Utc;
    use validatorforge_types::{NodeId, RunId};

    fn ctx(agent: MockNodeAgent) -> SagaContext {
        SagaContext {
            agent: Arc::new(agent),
            host: HostAddr::new("val01").unwrap(),
            version: ValidatorVersion::new("2.0.14").unwrap(),
            spare_host: Some(HostAddr::new("spare01").unwrap()),
        }
    }

    fn executor() -> SagaExecutor {
        let mut events = MockEventSink::new();
        events.expect_publish().returning(|_| ());
        let mut clock = MockClock::new();
        clock.expect_now().returning(Utc::now);
        SagaExecutor::new(Arc::new(events), Arc::new(clock))
    }

    fn run() -> DeploymentRun {
        DeploymentRun::new(
            RunId(1),
            NodeId::new("n1").unwrap(),
            DeploymentKind::Provision,
            Utc::now(),
        )
    }

    #[tokio::test]
    async fn provision_happy_path_succeeds() {
        let mut agent = MockNodeAgent::new();
        agent.expect_apply_infra().returning(|_| Ok(()));
        agent.expect_tune_host().returning(|_| Ok(()));
        agent.expect_start_validator().returning(|_, _| Ok(()));
        agent
            .expect_await_catchup()
            .returning(|_| Ok(validatorforge_types::Slot(100)));
        let steps = build_plan(&DeploymentKind::Provision);
        let mut r = run();
        let status = executor().run(&steps, &ctx(agent), &mut r).await;
        assert_eq!(status, RunStatus::Succeeded);
        assert_eq!(r.completed_steps().len(), 4);
    }

    #[tokio::test]
    async fn failure_triggers_compensation_rollback() {
        let mut agent = MockNodeAgent::new();
        agent.expect_apply_infra().returning(|_| Ok(()));
        agent.expect_tune_host().returning(|_| Ok(()));
        // start_validator fails -> compensate apply_infra (destroy) + tune (noop).
        agent
            .expect_start_validator()
            .returning(|_, _| Err(PortError::Rejected("bad binary".into())));
        agent.expect_drain().returning(|_| Ok(())); // compensation for start step (not reached as completed)
        agent.expect_destroy_infra().returning(|_| Ok(()));
        let steps = build_plan(&DeploymentKind::Provision);
        let mut r = run();
        let status = executor().run(&steps, &ctx(agent), &mut r).await;
        assert_eq!(status, RunStatus::RolledBack);
    }

    #[tokio::test]
    async fn compensation_failure_yields_failed() {
        let mut agent = MockNodeAgent::new();
        agent.expect_apply_infra().returning(|_| Ok(()));
        agent.expect_tune_host().returning(|_| Ok(()));
        agent
            .expect_start_validator()
            .returning(|_, _| Err(PortError::Rejected("bad binary".into())));
        // Compensation of apply_infra (destroy) fails -> Failed.
        agent
            .expect_destroy_infra()
            .returning(|_| Err(PortError::Internal("disk stuck".into())));
        let steps = build_plan(&DeploymentKind::Provision);
        let mut r = run();
        let status = executor().run(&steps, &ctx(agent), &mut r).await;
        assert_eq!(status, RunStatus::Failed);
    }

    #[tokio::test]
    async fn failover_plan_swaps_identity() {
        let mut agent = MockNodeAgent::new();
        agent.expect_drain().returning(|_| Ok(()));
        agent.expect_swap_identity().returning(|_, _| Ok(()));
        let kind = DeploymentKind::Failover {
            spare: NodeId::new("spare01").unwrap(),
        };
        let steps = build_plan(&kind);
        assert_eq!(steps.len(), 2);
        let mut r = run();
        let status = executor().run(&steps, &ctx(agent), &mut r).await;
        assert_eq!(status, RunStatus::Succeeded);
    }

    #[test]
    fn plans_have_expected_shapes() {
        assert_eq!(build_plan(&DeploymentKind::Provision).len(), 4);
        assert_eq!(build_plan(&DeploymentKind::Decommission).len(), 2);
    }
}
