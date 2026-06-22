//! Validator node aggregate and its lifecycle state machine.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::DomainError;
use crate::ids::{ClusterName, HostAddr, NodeId, ValidatorVersion};

/// Which Solana cluster a node participates in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cluster {
    /// Mainnet-beta.
    Mainnet,
    /// Testnet.
    Testnet,
    /// Devnet.
    Devnet,
}

impl Cluster {
    /// Stable wire string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Cluster::Mainnet => "mainnet",
            Cluster::Testnet => "testnet",
            Cluster::Devnet => "devnet",
        }
    }
}

impl std::fmt::Display for Cluster {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The operational role a node plays in the fleet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeRole {
    /// A staked, voting validator.
    Voting,
    /// An RPC node (non-voting, serves queries).
    Rpc,
    /// An unstaked hot-spare kept warm for fast identity failover.
    HotSpare,
}

impl NodeRole {
    /// Stable wire string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            NodeRole::Voting => "voting",
            NodeRole::Rpc => "rpc",
            NodeRole::HotSpare => "hot_spare",
        }
    }
}

impl std::fmt::Display for NodeRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The lifecycle state of a validator node.
///
/// Transitions are constrained by [`NodeState::can_transition_to`]; the engine
/// refuses any move not encoded here, so a node can never silently end up in an
/// inconsistent operational state. The legal graph is:
///
/// ```text
/// Provisioning ─▶ Bootstrapping ─▶ CatchingUp ─▶ Active ─▶ Delinquent
///       │               │              │           │          │
///       ▼               ▼              ▼           ▼          ▼
///     Failed          Failed         Failed     Draining   CatchingUp (recover)
///                                                  │           │
///                                                  ▼           ▼
///                                            Decommissioned   Active
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeState {
    /// Host is being provisioned (Terraform apply, OS tuning via Ansible).
    Provisioning,
    /// Validator process starting; downloading snapshot.
    Bootstrapping,
    /// Replaying ledger / catching up to the cluster tip.
    CatchingUp,
    /// Caught up and (for voting nodes) voting normally.
    Active,
    /// Was active but is now missing votes / delinquent.
    Delinquent,
    /// Being gracefully drained ahead of an upgrade or failover.
    Draining,
    /// Permanently removed from the fleet.
    Decommissioned,
    /// Entered a terminal failure during provisioning/bootstrap.
    Failed,
}

impl NodeState {
    /// Stable wire string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            NodeState::Provisioning => "provisioning",
            NodeState::Bootstrapping => "bootstrapping",
            NodeState::CatchingUp => "catching_up",
            NodeState::Active => "active",
            NodeState::Delinquent => "delinquent",
            NodeState::Draining => "draining",
            NodeState::Decommissioned => "decommissioned",
            NodeState::Failed => "failed",
        }
    }

    /// Whether this is a terminal state (no further transitions).
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, NodeState::Decommissioned | NodeState::Failed)
    }

    /// Whether a node in this state is serving / voting.
    #[must_use]
    pub const fn is_healthy(self) -> bool {
        matches!(self, NodeState::Active)
    }

    /// Whether the transition `self -> next` is permitted by the state machine.
    #[must_use]
    pub const fn can_transition_to(self, next: NodeState) -> bool {
        use NodeState::{
            Active, Bootstrapping, CatchingUp, Decommissioned, Delinquent, Draining, Failed,
            Provisioning,
        };
        matches!(
            (self, next),
            (Provisioning, Bootstrapping)
                | (Provisioning, Failed)
                | (Bootstrapping, CatchingUp)
                | (Bootstrapping, Failed)
                | (CatchingUp, Active)
                | (CatchingUp, Failed)
                | (Active, Delinquent)
                | (Active, Draining)
                | (Delinquent, CatchingUp)
                | (Delinquent, Draining)
                | (Draining, Decommissioned)
                | (Draining, CatchingUp)
        )
    }
}

impl std::fmt::Display for NodeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A validator node aggregate: identity, placement, role, version and current
/// lifecycle state. Fields are private; mutation only happens through
/// [`ValidatorNode::transition_to`], which enforces the state machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidatorNode {
    id: NodeId,
    cluster_name: ClusterName,
    cluster: Cluster,
    host: HostAddr,
    role: NodeRole,
    version: ValidatorVersion,
    state: NodeState,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl ValidatorNode {
    /// Register a brand-new node; it always starts in [`NodeState::Provisioning`].
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: NodeId,
        cluster_name: ClusterName,
        cluster: Cluster,
        host: HostAddr,
        role: NodeRole,
        version: ValidatorVersion,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            cluster_name,
            cluster,
            host,
            role,
            version,
            state: NodeState::Provisioning,
            created_at: now,
            updated_at: now,
        }
    }

    /// The node id.
    #[must_use]
    pub fn id(&self) -> &NodeId {
        &self.id
    }

    /// The cluster label.
    #[must_use]
    pub fn cluster_name(&self) -> &ClusterName {
        &self.cluster_name
    }

    /// The cluster.
    #[must_use]
    pub fn cluster(&self) -> Cluster {
        self.cluster
    }

    /// The host address.
    #[must_use]
    pub fn host(&self) -> &HostAddr {
        &self.host
    }

    /// The node role.
    #[must_use]
    pub fn role(&self) -> NodeRole {
        self.role
    }

    /// The currently deployed version.
    #[must_use]
    pub fn version(&self) -> &ValidatorVersion {
        &self.version
    }

    /// The current lifecycle state.
    #[must_use]
    pub fn state(&self) -> NodeState {
        self.state
    }

    /// When the node was first registered.
    #[must_use]
    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    /// When the node was last mutated.
    #[must_use]
    pub fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }

    /// Attempt a lifecycle transition, enforcing the state machine.
    ///
    /// # Errors
    /// Returns [`DomainError::IllegalTransition`] when the move is not allowed.
    pub fn transition_to(
        &mut self,
        next: NodeState,
        now: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        if !self.state.can_transition_to(next) {
            return Err(DomainError::IllegalTransition {
                from: self.state.as_str(),
                to: next.as_str(),
            });
        }
        self.state = next;
        self.updated_at = now;
        Ok(())
    }

    /// Record a new deployed version (used after a successful upgrade saga).
    pub fn set_version(&mut self, version: ValidatorVersion, now: DateTime<Utc>) {
        self.version = version;
        self.updated_at = now;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(now: DateTime<Utc>) -> ValidatorNode {
        ValidatorNode::new(
            NodeId::new("eu-val-01").unwrap(),
            ClusterName::new("eu-central-fiber").unwrap(),
            Cluster::Testnet,
            HostAddr::new("val01.internal").unwrap(),
            NodeRole::Voting,
            ValidatorVersion::new("2.0.14").unwrap(),
            now,
        )
    }

    #[test]
    fn new_node_starts_provisioning() {
        let n = sample(Utc::now());
        assert_eq!(n.state(), NodeState::Provisioning);
        assert_eq!(n.role(), NodeRole::Voting);
        assert_eq!(n.cluster(), Cluster::Testnet);
    }

    #[test]
    fn happy_path_lifecycle() {
        let now = Utc::now();
        let mut n = sample(now);
        for s in [
            NodeState::Bootstrapping,
            NodeState::CatchingUp,
            NodeState::Active,
        ] {
            n.transition_to(s, Utc::now()).unwrap();
        }
        assert!(n.state().is_healthy());
    }

    #[test]
    fn illegal_transition_is_rejected() {
        let mut n = sample(Utc::now());
        let err = n.transition_to(NodeState::Active, Utc::now()).unwrap_err();
        assert_eq!(err.code(), crate::ErrorCode::IllegalTransition);
        // State is unchanged after a rejected transition.
        assert_eq!(n.state(), NodeState::Provisioning);
    }

    #[test]
    fn delinquent_can_recover_or_drain() {
        assert!(NodeState::Active.can_transition_to(NodeState::Delinquent));
        assert!(NodeState::Delinquent.can_transition_to(NodeState::CatchingUp));
        assert!(NodeState::Delinquent.can_transition_to(NodeState::Draining));
    }

    #[test]
    fn terminal_states_have_no_exit() {
        for s in [NodeState::Decommissioned, NodeState::Failed] {
            assert!(s.is_terminal());
            for t in [
                NodeState::Provisioning,
                NodeState::Active,
                NodeState::CatchingUp,
            ] {
                assert!(!s.can_transition_to(t));
            }
        }
    }

    #[test]
    fn set_version_updates_timestamp() {
        let now = Utc::now();
        let mut n = sample(now);
        let later = now + chrono::Duration::seconds(5);
        n.set_version(ValidatorVersion::new("2.1.0").unwrap(), later);
        assert_eq!(n.version().as_str(), "2.1.0");
        assert_eq!(n.updated_at(), later);
    }

    #[test]
    fn enum_wire_strings_are_stable() {
        assert_eq!(Cluster::Mainnet.as_str(), "mainnet");
        assert_eq!(NodeRole::HotSpare.as_str(), "hot_spare");
        assert_eq!(NodeState::CatchingUp.as_str(), "catching_up");
    }
}
