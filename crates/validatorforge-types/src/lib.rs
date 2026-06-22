//! Pure domain types for ValidatorForge.
//!
//! This crate is the innermost layer of the hexagonal architecture: it owns the
//! vocabulary of the domain (validator-node identities, the lifecycle state
//! machine, health snapshots and ops/deployment value objects) and has **no**
//! dependency on any runtime, web framework, or database. Everything here is
//! deterministic and trivially testable.
//!
//! The headline design choice is that *illegal states are unrepresentable*:
//! [`NodeState`] transitions are validated by [`NodeState::can_transition_to`],
//! identities are [newtypes](ids) that validate on construction, and value
//! objects expose private fields behind getters so they cannot be mutated into
//! an inconsistent shape after validation.

#![forbid(unsafe_code)]

mod error;
mod health;
mod ids;
mod node;
mod ops;

pub use error::{DomainError, ErrorCode};
pub use health::{HealthSnapshot, HealthStatus};
pub use ids::{ClusterName, HostAddr, NodeId, RunId, Slot, ValidatorVersion};
pub use node::{Cluster, NodeRole, NodeState, ValidatorNode};
pub use ops::{DeploymentKind, DeploymentRun, OpsActionKind, RunStatus, SagaStepOutcome};
