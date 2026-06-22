//! In-memory implementations of the repository port. Thread-safe via `DashMap`
//! and a `parking_lot` mutex; suitable for the simulator binary and tests. The
//! durable Postgres variant lives in `pg` behind the `postgres` feature.

use async_trait::async_trait;
use dashmap::DashMap;
use parking_lot::Mutex;
use validatorforge_core::{NodeRepository, PortError};
use validatorforge_types::{DeploymentRun, NodeId, RunId, ValidatorNode};

/// A process-local node + run store.
#[derive(Default)]
pub struct InMemoryNodeRepository {
    nodes: DashMap<String, ValidatorNode>,
    runs: Mutex<Vec<DeploymentRun>>,
}

impl InMemoryNodeRepository {
    /// Create an empty repository.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of stored nodes.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
}

#[async_trait]
impl NodeRepository for InMemoryNodeRepository {
    async fn save_node(&self, node: &ValidatorNode) -> Result<(), PortError> {
        self.nodes.insert(node.id().to_string(), node.clone());
        Ok(())
    }

    async fn get_node(&self, id: &NodeId) -> Result<Option<ValidatorNode>, PortError> {
        Ok(self.nodes.get(&id.to_string()).map(|n| n.clone()))
    }

    async fn list_nodes(&self) -> Result<Vec<ValidatorNode>, PortError> {
        Ok(self.nodes.iter().map(|e| e.value().clone()).collect())
    }

    async fn save_run(&self, run: &DeploymentRun) -> Result<(), PortError> {
        let mut runs = self.runs.lock();
        if let Some(existing) = runs.iter_mut().find(|r| r.id() == run.id()) {
            *existing = run.clone();
        } else {
            runs.push(run.clone());
        }
        Ok(())
    }

    async fn get_run(&self, id: RunId) -> Result<Option<DeploymentRun>, PortError> {
        Ok(self.runs.lock().iter().find(|r| r.id() == id).cloned())
    }

    async fn list_runs(&self, limit: usize) -> Result<Vec<DeploymentRun>, PortError> {
        let runs = self.runs.lock();
        let mut out: Vec<DeploymentRun> = runs.clone();
        out.sort_by_key(|b| std::cmp::Reverse(b.started_at()));
        out.truncate(limit);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use validatorforge_types::{
        Cluster, ClusterName, DeploymentKind, HostAddr, NodeRole, ValidatorVersion,
    };

    fn node(id: &str) -> ValidatorNode {
        ValidatorNode::new(
            NodeId::new(id).unwrap(),
            ClusterName::new("eu-fiber").unwrap(),
            Cluster::Testnet,
            HostAddr::new("val.internal").unwrap(),
            NodeRole::Voting,
            ValidatorVersion::new("2.0.14").unwrap(),
            Utc::now(),
        )
    }

    #[tokio::test]
    async fn save_and_get_node() {
        let repo = InMemoryNodeRepository::new();
        repo.save_node(&node("n1")).await.unwrap();
        let got = repo.get_node(&NodeId::new("n1").unwrap()).await.unwrap();
        assert!(got.is_some());
        assert_eq!(repo.node_count(), 1);
    }

    #[tokio::test]
    async fn missing_node_is_none() {
        let repo = InMemoryNodeRepository::new();
        let got = repo.get_node(&NodeId::new("ghost").unwrap()).await.unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn runs_listed_newest_first_and_capped() {
        let repo = InMemoryNodeRepository::new();
        for i in 0i64..5 {
            let r = DeploymentRun::new(
                RunId(i as u128),
                NodeId::new("n1").unwrap(),
                DeploymentKind::Provision,
                Utc::now() + chrono::Duration::seconds(i),
            );
            repo.save_run(&r).await.unwrap();
        }
        let listed = repo.list_runs(3).await.unwrap();
        assert_eq!(listed.len(), 3);
        // Newest (largest started_at) first.
        assert_eq!(listed[0].id(), RunId(4));
    }

    #[tokio::test]
    async fn save_run_is_upsert() {
        let repo = InMemoryNodeRepository::new();
        let mut r = DeploymentRun::new(
            RunId(1),
            NodeId::new("n1").unwrap(),
            DeploymentKind::Provision,
            Utc::now(),
        );
        repo.save_run(&r).await.unwrap();
        r.finish(validatorforge_types::RunStatus::Succeeded, Utc::now());
        repo.save_run(&r).await.unwrap();
        let got = repo.get_run(RunId(1)).await.unwrap().unwrap();
        assert_eq!(got.status(), validatorforge_types::RunStatus::Succeeded);
        assert_eq!(repo.list_runs(10).await.unwrap().len(), 1);
    }
}
