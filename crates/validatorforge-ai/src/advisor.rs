//! Deterministic, always-available ops advisor.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use validatorforge_core::OpsAdvisor;
use validatorforge_types::{NodeRole, NodeState, ValidatorNode};

/// The concrete next action the advisor recommends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecommendedAction {
    /// No action; the node is healthy or already progressing.
    Hold,
    /// A human (or automation) should investigate before acting.
    Investigate,
    /// Tune the host OS (lag attributable to resource pressure).
    TuneHost,
    /// Fail the staked identity over to a hot spare.
    Failover,
    /// Roll the node forward to a newer validator version.
    Upgrade,
    /// Drain and decommission the node.
    Decommission,
}

impl RecommendedAction {
    /// Stable wire string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            RecommendedAction::Hold => "hold",
            RecommendedAction::Investigate => "investigate",
            RecommendedAction::TuneHost => "tune_host",
            RecommendedAction::Failover => "failover",
            RecommendedAction::Upgrade => "upgrade",
            RecommendedAction::Decommission => "decommission",
        }
    }
}

/// How urgently the recommendation should be acted on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Urgency {
    /// Informational; act at leisure.
    Low,
    /// Act within the next maintenance window.
    Medium,
    /// Act now — stake/rewards at risk.
    High,
}

/// A structured recommendation. Serialised to JSON for the [`OpsAdvisor`] port.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recommendation {
    /// The recommended action.
    pub action: RecommendedAction,
    /// How urgent it is.
    pub urgency: Urgency,
    /// Human-readable justification.
    pub rationale: String,
}

/// A pure recommender driven by the node's state + a free-text context signal.
#[derive(Debug, Clone, Copy, Default)]
pub struct HeuristicAdvisor;

impl HeuristicAdvisor {
    /// Construct the advisor.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Produce a structured recommendation for `node` given `context`.
    ///
    /// `context` is a lower-cased free-text signal (e.g. an alert summary). The
    /// rules below are intentionally explicit and ordered by severity so the
    /// output is fully deterministic and unit-testable.
    #[must_use]
    pub fn recommend(&self, node: &ValidatorNode, context: &str) -> Recommendation {
        let ctx = context.to_ascii_lowercase();
        let state = node.state();

        // 1) Terminal failure: needs a human before anything automated.
        if state == NodeState::Failed {
            return Recommendation {
                action: RecommendedAction::Investigate,
                urgency: Urgency::High,
                rationale: "node is in a terminal Failed state; inspect provisioning/bootstrap \
                            logs before retrying"
                    .into(),
            };
        }

        // 2) Delinquency (state or context): for a voting node, fail over fast.
        let delinquent = state == NodeState::Delinquent
            || ctx.contains("delinquent")
            || ctx.contains("not voting");
        if delinquent {
            return if node.role() == NodeRole::Voting {
                Recommendation {
                    action: RecommendedAction::Failover,
                    urgency: Urgency::High,
                    rationale: "voting node is delinquent; fail the staked identity over to a hot \
                                spare to stop missing votes"
                        .into(),
                }
            } else {
                Recommendation {
                    action: RecommendedAction::Investigate,
                    urgency: Urgency::Medium,
                    rationale: "non-voting node is delinquent; no stake at risk, investigate \
                                connectivity"
                        .into(),
                }
            };
        }

        // 3) Explicit upgrade request.
        if ctx.contains("upgrade") || ctx.contains("new version") || ctx.contains("cve") {
            return Recommendation {
                action: RecommendedAction::Upgrade,
                urgency: if ctx.contains("cve") {
                    Urgency::High
                } else {
                    Urgency::Medium
                },
                rationale: "a newer/secure version is indicated; run the zero-downtime upgrade \
                            saga (drain → restart → catch up)"
                    .into(),
            };
        }

        // 4) Lag while otherwise active: likely host resource pressure.
        if (ctx.contains("lag") || ctx.contains("behind") || ctx.contains("slow"))
            && state.is_healthy()
        {
            return Recommendation {
                action: RecommendedAction::TuneHost,
                urgency: Urgency::Medium,
                rationale: "node is active but lagging; apply host tuning (hugepages, CPU \
                            governor, NIC) and re-evaluate"
                    .into(),
            };
        }

        // 5) In-flight lifecycle states: let the saga finish.
        if matches!(
            state,
            NodeState::Provisioning | NodeState::Bootstrapping | NodeState::CatchingUp
        ) {
            return Recommendation {
                action: RecommendedAction::Hold,
                urgency: Urgency::Low,
                rationale: "node is mid-lifecycle; allow the in-flight run to complete before \
                            intervening"
                    .into(),
            };
        }

        // 6) Draining or decommissioned: nothing to do.
        if matches!(state, NodeState::Draining | NodeState::Decommissioned) {
            return Recommendation {
                action: RecommendedAction::Hold,
                urgency: Urgency::Low,
                rationale: "node is draining/decommissioned; no action required".into(),
            };
        }

        // 7) Default: healthy and quiet.
        Recommendation {
            action: RecommendedAction::Hold,
            urgency: Urgency::Low,
            rationale: "node is active and within tolerances; hold".into(),
        }
    }
}

#[async_trait]
impl OpsAdvisor for HeuristicAdvisor {
    async fn advise(&self, node: &ValidatorNode, context: &str) -> String {
        let rec = self.recommend(node, context);
        serde_json::to_string(&rec)
            .unwrap_or_else(|_| format!("{}: {}", rec.action.as_str(), rec.rationale))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};
    use validatorforge_types::{Cluster, ClusterName, HostAddr, NodeId, ValidatorVersion};

    fn node_in(state: NodeState, role: NodeRole) -> ValidatorNode {
        let now = Utc::now();
        let mut n = ValidatorNode::new(
            NodeId::new("eu-val-01").unwrap(),
            ClusterName::new("eu-fiber").unwrap(),
            Cluster::Mainnet,
            HostAddr::new("val01.internal").unwrap(),
            role,
            ValidatorVersion::new("2.0.14").unwrap(),
            now,
        );
        drive_to(&mut n, state, now);
        n
    }

    fn drive_to(n: &mut ValidatorNode, target: NodeState, now: DateTime<Utc>) {
        let path: &[NodeState] = match target {
            NodeState::Provisioning => &[],
            NodeState::Bootstrapping => &[NodeState::Bootstrapping],
            NodeState::CatchingUp => &[NodeState::Bootstrapping, NodeState::CatchingUp],
            NodeState::Active => &[
                NodeState::Bootstrapping,
                NodeState::CatchingUp,
                NodeState::Active,
            ],
            NodeState::Delinquent => &[
                NodeState::Bootstrapping,
                NodeState::CatchingUp,
                NodeState::Active,
                NodeState::Delinquent,
            ],
            NodeState::Draining => &[
                NodeState::Bootstrapping,
                NodeState::CatchingUp,
                NodeState::Active,
                NodeState::Draining,
            ],
            NodeState::Decommissioned => &[
                NodeState::Bootstrapping,
                NodeState::CatchingUp,
                NodeState::Active,
                NodeState::Draining,
                NodeState::Decommissioned,
            ],
            NodeState::Failed => &[NodeState::Failed],
        };
        for s in path {
            n.transition_to(*s, now).unwrap();
        }
    }

    #[test]
    fn failed_node_is_investigate_high() {
        let n = node_in(NodeState::Failed, NodeRole::Voting);
        let r = HeuristicAdvisor::new().recommend(&n, "");
        assert_eq!(r.action, RecommendedAction::Investigate);
        assert_eq!(r.urgency, Urgency::High);
    }

    #[test]
    fn delinquent_voting_node_fails_over() {
        let n = node_in(NodeState::Delinquent, NodeRole::Voting);
        let r = HeuristicAdvisor::new().recommend(&n, "missing votes");
        assert_eq!(r.action, RecommendedAction::Failover);
        assert_eq!(r.urgency, Urgency::High);
    }

    #[test]
    fn delinquent_rpc_node_only_investigates() {
        let n = node_in(NodeState::Delinquent, NodeRole::Rpc);
        let r = HeuristicAdvisor::new().recommend(&n, "");
        assert_eq!(r.action, RecommendedAction::Investigate);
    }

    #[test]
    fn cve_context_triggers_urgent_upgrade() {
        let n = node_in(NodeState::Active, NodeRole::Voting);
        let r = HeuristicAdvisor::new().recommend(&n, "CVE-2025-1234 disclosed");
        assert_eq!(r.action, RecommendedAction::Upgrade);
        assert_eq!(r.urgency, Urgency::High);
    }

    #[test]
    fn lagging_active_node_tunes_host() {
        let n = node_in(NodeState::Active, NodeRole::Voting);
        let r = HeuristicAdvisor::new().recommend(&n, "node falling behind tip");
        assert_eq!(r.action, RecommendedAction::TuneHost);
    }

    #[test]
    fn in_flight_node_holds() {
        let n = node_in(NodeState::CatchingUp, NodeRole::Voting);
        let r = HeuristicAdvisor::new().recommend(&n, "");
        assert_eq!(r.action, RecommendedAction::Hold);
        assert_eq!(r.urgency, Urgency::Low);
    }

    #[tokio::test]
    async fn advise_returns_json() {
        let n = node_in(NodeState::Active, NodeRole::Voting);
        let out = HeuristicAdvisor::new().advise(&n, "all good").await;
        assert!(out.contains("\"action\""));
        assert!(out.contains("hold"));
    }
}
