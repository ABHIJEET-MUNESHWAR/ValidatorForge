//! Domain events emitted by the ops engine (event-driven backbone).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use validatorforge_types::{
    DeploymentKind, HealthStatus, NodeId, NodeState, OpsActionKind, RunId, RunStatus,
};

/// An event published whenever the control plane changes fleet state.
///
/// Subscribers (the GraphQL subscription, metrics bridges, future Kafka outbox)
/// consume these without coupling to the engine internals.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum OpsEvent {
    /// A new orchestration run has begun.
    RunStarted {
        /// The run id.
        run: RunId,
        /// The targeted node.
        node: NodeId,
        /// The kind of deployment.
        kind: DeploymentKind,
        /// When it started.
        at: DateTime<Utc>,
    },
    /// A saga step finished its forward action successfully.
    StepCompleted {
        /// The run id.
        run: RunId,
        /// The action that completed.
        action: OpsActionKind,
        /// When it completed.
        at: DateTime<Utc>,
    },
    /// A saga step was compensated (rolled back) after a failure.
    StepCompensated {
        /// The run id.
        run: RunId,
        /// The action that was compensated.
        action: OpsActionKind,
        /// When it was compensated.
        at: DateTime<Utc>,
    },
    /// A node moved between lifecycle states.
    NodeStateChanged {
        /// The node id.
        node: NodeId,
        /// Previous state.
        from: NodeState,
        /// New state.
        to: NodeState,
        /// When it changed.
        at: DateTime<Utc>,
    },
    /// A health snapshot was evaluated.
    HealthEvaluated {
        /// The node id.
        node: NodeId,
        /// The resulting status.
        status: HealthStatus,
        /// When it was evaluated.
        at: DateTime<Utc>,
    },
    /// An orchestration run reached a terminal status.
    RunFinished {
        /// The run id.
        run: RunId,
        /// Final status.
        status: RunStatus,
        /// When it finished.
        at: DateTime<Utc>,
    },
}

impl OpsEvent {
    /// The event's discriminant string (for metrics labels).
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            OpsEvent::RunStarted { .. } => "run_started",
            OpsEvent::StepCompleted { .. } => "step_completed",
            OpsEvent::StepCompensated { .. } => "step_compensated",
            OpsEvent::NodeStateChanged { .. } => "node_state_changed",
            OpsEvent::HealthEvaluated { .. } => "health_evaluated",
            OpsEvent::RunFinished { .. } => "run_finished",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_kind_strings() {
        let e = OpsEvent::RunFinished {
            run: RunId(1),
            status: RunStatus::Succeeded,
            at: Utc::now(),
        };
        assert_eq!(e.kind(), "run_finished");
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("run_finished"));
    }
}
