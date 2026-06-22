//! `validatorforge-ai` — the agentic ops advisor.
//!
//! - [`advisor`] — the deterministic [`advisor::HeuristicAdvisor`]: a pure,
//!   always-available recommender that maps a node's lifecycle state, role and a
//!   free-text context signal to a [`advisor::Recommendation`].
//! - [`llm`] (feature `llm`) — an LLM-backed explainer that wraps an
//!   OpenAI-compatible endpoint with timeout + bounded retry and **degrades to
//!   the heuristic on any failure**, so the network is never on the critical
//!   path for a recommendation.
//!
//! Both implement the [`validatorforge_core::OpsAdvisor`] port, so the API and
//! node layers depend only on the trait.

#![forbid(unsafe_code)]

pub mod advisor;

#[cfg(feature = "llm")]
pub mod llm;

pub use advisor::{HeuristicAdvisor, Recommendation, RecommendedAction, Urgency};

#[cfg(feature = "llm")]
pub use llm::{LlmAdvisor, LlmConfig};
