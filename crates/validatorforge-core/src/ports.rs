//! Outbound ports (hexagonal boundaries). Adapters live in `validatorforge-infra`.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use validatorforge_types::{
    DeploymentKind, DeploymentRun, HostAddr, NodeId, OpsActionKind, RunId, Slot, ValidatorNode,
    ValidatorVersion,
};

use crate::error::PortError;
use crate::event::OpsEvent;

/// Wall-clock source (domain timestamps). Separate from the resilience monotonic
/// clock so domain time can be frozen in tests independently of backoff timing.
#[cfg_attr(test, mockall::automock)]
pub trait Clock: Send + Sync {
    /// The current wall-clock time.
    fn now(&self) -> DateTime<Utc>;
}

/// Effectful operations performed against a single node host. Implementations
/// shell out to Ansible/Terraform/SSH/RPC; the simulator implements them in-memory.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait NodeAgent: Send + Sync {
    /// Render and apply infrastructure (Terraform apply + base Ansible).
    async fn apply_infra(&self, host: &HostAddr) -> Result<(), PortError>;
    /// Apply OS performance tuning (sysctl, hugepages, CPU pinning, NIC).
    async fn tune_host(&self, host: &HostAddr) -> Result<(), PortError>;
    /// Start the validator process and fetch a snapshot.
    async fn start_validator(
        &self,
        host: &HostAddr,
        version: &ValidatorVersion,
    ) -> Result<(), PortError>;
    /// Block until the node has caught up to the cluster tip; returns the tip slot.
    async fn await_catchup(&self, host: &HostAddr) -> Result<Slot, PortError>;
    /// Gracefully drain (stop voting, swap to a junk identity).
    async fn drain(&self, host: &HostAddr) -> Result<(), PortError>;
    /// Move the staked identity from `from` onto `to`.
    async fn swap_identity(&self, from: &HostAddr, to: &HostAddr) -> Result<(), PortError>;
    /// Destroy the host infrastructure (Terraform destroy).
    async fn destroy_infra(&self, host: &HostAddr) -> Result<(), PortError>;
}

/// Persistence for the node read model and the run audit trail.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait NodeRepository: Send + Sync {
    /// Upsert a node.
    async fn save_node(&self, node: &ValidatorNode) -> Result<(), PortError>;
    /// Fetch a node by id.
    async fn get_node(&self, id: &NodeId) -> Result<Option<ValidatorNode>, PortError>;
    /// List all nodes.
    async fn list_nodes(&self) -> Result<Vec<ValidatorNode>, PortError>;
    /// Upsert a deployment run.
    async fn save_run(&self, run: &DeploymentRun) -> Result<(), PortError>;
    /// Fetch a run by id.
    async fn get_run(&self, id: RunId) -> Result<Option<DeploymentRun>, PortError>;
    /// List recent runs, newest first, capped at `limit`.
    async fn list_runs(&self, limit: usize) -> Result<Vec<DeploymentRun>, PortError>;
}

/// Publishes [`OpsEvent`]s to subscribers.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait EventSink: Send + Sync {
    /// Publish one event (best-effort; never fails the caller's operation).
    async fn publish(&self, event: OpsEvent);
}

/// A subscribable source of [`OpsEvent`]s (read side of the event bus, used by
/// the GraphQL subscription). Kept separate from [`EventSink`] so the write path
/// and the streaming read path are independent capabilities.
pub trait EventStream: Send + Sync {
    /// Obtain an independent live stream of events.
    fn subscribe(&self) -> futures::stream::BoxStream<'static, OpsEvent>;
}

/// Renders infrastructure-as-code artifacts for a deployment. Pure/synchronous.
pub trait IacRenderer: Send + Sync {
    /// Render a Terraform plan snippet for the given node + action.
    fn render_terraform(&self, node: &ValidatorNode, kind: &DeploymentKind) -> String;
    /// Render an Ansible playbook snippet for the given step.
    fn render_ansible(&self, node: &ValidatorNode, action: OpsActionKind) -> String;
}

/// Advises on the next ops action for a node (Generative/Agentic AI port).
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait OpsAdvisor: Send + Sync {
    /// Produce a human-readable recommendation for the given node + context.
    async fn advise(&self, node: &ValidatorNode, context: &str) -> String;
}
