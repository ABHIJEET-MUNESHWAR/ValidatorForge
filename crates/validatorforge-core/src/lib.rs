//! Application core: orchestration logic with no concrete I/O.
//!
//! The core depends only on [`validatorforge_types`] (domain) and
//! [`validatorforge_resilience`] (primitives). All outbound effects are
//! expressed as **ports** (traits) that adapters in `validatorforge-infra`
//! implement. The two headline pieces are:
//!
//! - [`ResilientNodeAgent`] — a decorator that wraps every [`NodeAgent`] call in
//!   timeout + retry + circuit-breaker + rate-limit (the *resilience on every
//!   boundary* rule).
//! - [`SagaExecutor`] — runs an ordered list of [`SagaStep`]s and, on failure,
//!   compensates the already-completed steps in reverse (the *Saga* pattern).

#![forbid(unsafe_code)]

mod agent;
mod config;
mod engine;
mod error;
mod event;
mod ports;
mod saga;

pub use agent::ResilientNodeAgent;
pub use config::OpsConfig;
pub use engine::OpsEngine;
pub use error::{CoreError, PortError};
pub use event::OpsEvent;
pub use ports::{
    Clock, EventSink, EventStream, IacRenderer, NodeAgent, NodeRepository, OpsAdvisor,
};
pub use saga::{build_plan, SagaContext, SagaExecutor, SagaStep};

#[cfg(test)]
pub use ports::{MockClock, MockEventSink, MockNodeAgent, MockNodeRepository, MockOpsAdvisor};
