//! Ops / deployment value objects: the kinds of orchestration the control plane
//! runs, and the run record that tracks a saga's progress.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ids::{NodeId, RunId, ValidatorVersion};

/// The kind of deployment orchestration a run performs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum DeploymentKind {
    /// Provision a brand-new node and bring it to `Active`.
    Provision,
    /// Zero-downtime upgrade to a target version via drain + hot-spare swap.
    Upgrade {
        /// The version to upgrade to.
        target_version: ValidatorVersion,
    },
    /// Fail the active identity over to a hot spare.
    Failover {
        /// The hot-spare node that should take over the identity.
        spare: NodeId,
    },
    /// Drain and decommission a node.
    Decommission,
}

impl DeploymentKind {
    /// Stable wire discriminant.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            DeploymentKind::Provision => "provision",
            DeploymentKind::Upgrade { .. } => "upgrade",
            DeploymentKind::Failover { .. } => "failover",
            DeploymentKind::Decommission => "decommission",
        }
    }
}

/// A discrete imperative action a saga step asks a node agent to perform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "action")]
pub enum OpsActionKind {
    /// Render + apply infrastructure (Terraform plan, Ansible playbook).
    ApplyInfra,
    /// Tune the host OS (sysctl, hugepages, CPU pinning, NIC).
    TuneHost,
    /// Start the validator process and fetch a snapshot.
    StartValidator,
    /// Wait until the node has caught up to the cluster tip.
    AwaitCatchup,
    /// Gracefully drain (stop voting, set identity to a junk keypair).
    Drain,
    /// Swap the staked identity onto the target node.
    SwapIdentity,
    /// Decommission the host (Terraform destroy).
    DestroyInfra,
}

impl OpsActionKind {
    /// Stable wire string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            OpsActionKind::ApplyInfra => "apply_infra",
            OpsActionKind::TuneHost => "tune_host",
            OpsActionKind::StartValidator => "start_validator",
            OpsActionKind::AwaitCatchup => "await_catchup",
            OpsActionKind::Drain => "drain",
            OpsActionKind::SwapIdentity => "swap_identity",
            OpsActionKind::DestroyInfra => "destroy_infra",
        }
    }
}

impl std::fmt::Display for OpsActionKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Outcome of a single saga step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SagaStepOutcome {
    /// The forward action succeeded.
    Completed,
    /// The forward action failed and was compensated (rolled back).
    Compensated,
    /// The step was skipped (precondition already satisfied).
    Skipped,
}

impl SagaStepOutcome {
    /// Stable wire string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            SagaStepOutcome::Completed => "completed",
            SagaStepOutcome::Compensated => "compensated",
            SagaStepOutcome::Skipped => "skipped",
        }
    }
}

/// Terminal/in-flight status of a whole orchestration run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    /// Saga is executing.
    Running,
    /// All steps completed successfully.
    Succeeded,
    /// A step failed and the saga compensated back to a safe state.
    RolledBack,
    /// A step failed and compensation also failed (needs human attention).
    Failed,
}

impl RunStatus {
    /// Stable wire string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            RunStatus::Running => "running",
            RunStatus::Succeeded => "succeeded",
            RunStatus::RolledBack => "rolled_back",
            RunStatus::Failed => "failed",
        }
    }

    /// Whether the run has reached a terminal status.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        !matches!(self, RunStatus::Running)
    }
}

impl std::fmt::Display for RunStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A record of one orchestration run (the saga's audit trail / read model).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentRun {
    id: RunId,
    target: NodeId,
    kind: DeploymentKind,
    status: RunStatus,
    completed_steps: Vec<OpsActionKind>,
    started_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
}

impl DeploymentRun {
    /// Open a new run in [`RunStatus::Running`].
    #[must_use]
    pub fn new(id: RunId, target: NodeId, kind: DeploymentKind, started_at: DateTime<Utc>) -> Self {
        Self {
            id,
            target,
            kind,
            status: RunStatus::Running,
            completed_steps: Vec::new(),
            started_at,
            finished_at: None,
        }
    }

    /// The run id.
    #[must_use]
    pub fn id(&self) -> RunId {
        self.id
    }

    /// The targeted node.
    #[must_use]
    pub fn target(&self) -> &NodeId {
        &self.target
    }

    /// The deployment kind.
    #[must_use]
    pub fn kind(&self) -> &DeploymentKind {
        &self.kind
    }

    /// Current status.
    #[must_use]
    pub fn status(&self) -> RunStatus {
        self.status
    }

    /// The steps completed so far (forward actions only).
    #[must_use]
    pub fn completed_steps(&self) -> &[OpsActionKind] {
        &self.completed_steps
    }

    /// When the run started.
    #[must_use]
    pub fn started_at(&self) -> DateTime<Utc> {
        self.started_at
    }

    /// When the run finished, if terminal.
    #[must_use]
    pub fn finished_at(&self) -> Option<DateTime<Utc>> {
        self.finished_at
    }

    /// Record a successfully completed forward step.
    pub fn record_step(&mut self, action: OpsActionKind) {
        self.completed_steps.push(action);
    }

    /// Mark the run terminal with the given status.
    pub fn finish(&mut self, status: RunStatus, now: DateTime<Utc>) {
        self.status = status;
        self.finished_at = Some(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deployment_kind_discriminants() {
        assert_eq!(DeploymentKind::Provision.as_str(), "provision");
        let up = DeploymentKind::Upgrade {
            target_version: ValidatorVersion::new("2.1.0").unwrap(),
        };
        assert_eq!(up.as_str(), "upgrade");
    }

    #[test]
    fn run_lifecycle() {
        let mut run = DeploymentRun::new(
            RunId(1),
            NodeId::new("n1").unwrap(),
            DeploymentKind::Provision,
            Utc::now(),
        );
        assert_eq!(run.status(), RunStatus::Running);
        assert!(!run.status().is_terminal());
        run.record_step(OpsActionKind::ApplyInfra);
        run.record_step(OpsActionKind::StartValidator);
        run.finish(RunStatus::Succeeded, Utc::now());
        assert!(run.status().is_terminal());
        assert_eq!(run.completed_steps().len(), 2);
        assert!(run.finished_at().is_some());
    }

    #[test]
    fn status_wire_strings() {
        assert_eq!(RunStatus::RolledBack.as_str(), "rolled_back");
        assert_eq!(SagaStepOutcome::Compensated.as_str(), "compensated");
        assert_eq!(OpsActionKind::SwapIdentity.as_str(), "swap_identity");
    }

    #[test]
    fn deployment_kind_serde_tagged() {
        let json = serde_json::to_string(&DeploymentKind::Decommission).unwrap();
        assert!(json.contains("decommission"));
    }
}
