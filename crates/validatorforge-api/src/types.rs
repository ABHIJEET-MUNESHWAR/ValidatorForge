//! GraphQL output/input types mapping the domain model to the API surface.

use async_graphql::{Enum, SimpleObject};
use chrono::{DateTime, Utc};
use validatorforge_core::OpsEvent;
use validatorforge_types::{Cluster, DeploymentRun, NodeRole, ValidatorNode};

/// A validator node as exposed over GraphQL.
#[derive(SimpleObject)]
pub struct NodeObject {
    /// Node id.
    pub id: String,
    /// Cluster (mainnet/testnet/devnet).
    pub cluster: String,
    /// Operator cluster label.
    pub cluster_name: String,
    /// Host address.
    pub host: String,
    /// Role (voting/rpc/hot_spare).
    pub role: String,
    /// Current lifecycle state.
    pub state: String,
    /// Deployed validator version.
    pub version: String,
    /// When the node was registered.
    pub created_at: DateTime<Utc>,
    /// When the node was last mutated.
    pub updated_at: DateTime<Utc>,
}

impl From<&ValidatorNode> for NodeObject {
    fn from(n: &ValidatorNode) -> Self {
        Self {
            id: n.id().to_string(),
            cluster: n.cluster().as_str().to_string(),
            cluster_name: n.cluster_name().to_string(),
            host: n.host().to_string(),
            role: n.role().as_str().to_string(),
            state: n.state().as_str().to_string(),
            version: n.version().to_string(),
            created_at: n.created_at(),
            updated_at: n.updated_at(),
        }
    }
}

/// A deployment run record as exposed over GraphQL.
#[derive(SimpleObject)]
pub struct RunObject {
    /// Run id.
    pub id: String,
    /// Targeted node id.
    pub target: String,
    /// Deployment kind.
    pub kind: String,
    /// Terminal/in-flight status.
    pub status: String,
    /// Forward steps completed, in order.
    pub completed_steps: Vec<String>,
    /// When the run started.
    pub started_at: DateTime<Utc>,
    /// When the run finished, if terminal.
    pub finished_at: Option<DateTime<Utc>>,
}

impl From<&DeploymentRun> for RunObject {
    fn from(r: &DeploymentRun) -> Self {
        Self {
            id: r.id().to_string(),
            target: r.target().to_string(),
            kind: r.kind().as_str().to_string(),
            status: r.status().as_str().to_string(),
            completed_steps: r
                .completed_steps()
                .iter()
                .map(|a| a.as_str().to_string())
                .collect(),
            started_at: r.started_at(),
            finished_at: r.finished_at(),
        }
    }
}

/// Service health summary.
#[derive(SimpleObject)]
pub struct HealthObject {
    /// Overall status string.
    pub status: String,
    /// Number of nodes currently tracked.
    pub nodes: i32,
}

/// A streamed ops event (kind + JSON payload).
#[derive(SimpleObject, Clone)]
pub struct OpsEventObject {
    /// Discriminant (e.g. `run_started`).
    pub kind: String,
    /// JSON-encoded event payload.
    pub payload: String,
}

impl From<OpsEvent> for OpsEventObject {
    fn from(e: OpsEvent) -> Self {
        let kind = e.kind().to_string();
        let payload = serde_json::to_string(&e).unwrap_or_else(|_| "{}".to_string());
        Self { kind, payload }
    }
}

/// GraphQL cluster enum (input).
#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum ClusterGql {
    /// Mainnet-beta.
    Mainnet,
    /// Testnet.
    Testnet,
    /// Devnet.
    Devnet,
}

impl From<ClusterGql> for Cluster {
    fn from(c: ClusterGql) -> Self {
        match c {
            ClusterGql::Mainnet => Cluster::Mainnet,
            ClusterGql::Testnet => Cluster::Testnet,
            ClusterGql::Devnet => Cluster::Devnet,
        }
    }
}

/// GraphQL node-role enum (input).
#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum NodeRoleGql {
    /// Staked, voting validator.
    Voting,
    /// RPC node.
    Rpc,
    /// Unstaked hot spare.
    HotSpare,
}

impl From<NodeRoleGql> for NodeRole {
    fn from(r: NodeRoleGql) -> Self {
        match r {
            NodeRoleGql::Voting => NodeRole::Voting,
            NodeRoleGql::Rpc => NodeRole::Rpc,
            NodeRoleGql::HotSpare => NodeRole::HotSpare,
        }
    }
}

/// GraphQL deployment-kind selector (input).
#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum DeploymentKindGql {
    /// Provision a new node.
    Provision,
    /// Zero-downtime upgrade (requires `targetVersion`).
    Upgrade,
    /// Fail over to a hot spare (requires `spare`).
    Failover,
    /// Drain and decommission.
    Decommission,
}
