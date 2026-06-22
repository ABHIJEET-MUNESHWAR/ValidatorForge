//! Mutation root — the write side: register nodes and launch deployment sagas.

use async_graphql::{Context, InputObject, Object, Result};
use chrono::Utc;
use validatorforge_types::{
    Cluster, ClusterName, DeploymentKind, HostAddr, NodeId, ValidatorNode, ValidatorVersion,
};

use crate::schema::{to_err, ApiContext};
use crate::types::{ClusterGql, DeploymentKindGql, NodeObject, NodeRoleGql, RunObject};

/// Input for [`MutationRoot::register_node`].
#[derive(InputObject)]
pub struct RegisterNodeInput {
    /// Unique node id (≤ 64 chars).
    pub id: String,
    /// Operator cluster label.
    pub cluster_name: String,
    /// Cluster.
    pub cluster: ClusterGql,
    /// Host address (≤ 253 chars).
    pub host: String,
    /// Role.
    pub role: NodeRoleGql,
    /// Validator version (e.g. `2.0.14`).
    pub version: String,
}

/// Input for [`MutationRoot::start_deployment`].
#[derive(InputObject)]
pub struct StartDeploymentInput {
    /// Target node id.
    pub node_id: String,
    /// What kind of deployment to run.
    pub kind: DeploymentKindGql,
    /// Target version (required when `kind = UPGRADE`).
    pub target_version: Option<String>,
    /// Spare node id (required when `kind = FAILOVER`).
    pub spare: Option<String>,
}

/// Write entry points.
pub struct MutationRoot;

#[Object]
impl MutationRoot {
    /// Register a new node (starts in `Provisioning`).
    async fn register_node(
        &self,
        ctx: &Context<'_>,
        input: RegisterNodeInput,
    ) -> Result<NodeObject> {
        let api = ctx.data_unchecked::<ApiContext>();
        let map = |e: validatorforge_types::DomainError| async_graphql::Error::new(e.to_string());

        let node = ValidatorNode::new(
            NodeId::new(input.id).map_err(map)?,
            ClusterName::new(input.cluster_name).map_err(map)?,
            Cluster::from(input.cluster),
            HostAddr::new(input.host).map_err(map)?,
            input.role.into(),
            ValidatorVersion::new(input.version).map_err(map)?,
            Utc::now(),
        );
        let view = NodeObject::from(&node);
        api.engine.register_node(node).await.map_err(to_err)?;
        Ok(view)
    }

    /// Launch a deployment saga and return the terminal run record.
    async fn start_deployment(
        &self,
        ctx: &Context<'_>,
        input: StartDeploymentInput,
    ) -> Result<RunObject> {
        let api = ctx.data_unchecked::<ApiContext>();
        let node_id =
            NodeId::new(input.node_id).map_err(|e| async_graphql::Error::new(e.to_string()))?;

        let kind = match input.kind {
            DeploymentKindGql::Provision => DeploymentKind::Provision,
            DeploymentKindGql::Decommission => DeploymentKind::Decommission,
            DeploymentKindGql::Upgrade => {
                let v = input.target_version.ok_or_else(|| {
                    async_graphql::Error::new("targetVersion required for UPGRADE")
                })?;
                DeploymentKind::Upgrade {
                    target_version: ValidatorVersion::new(v)
                        .map_err(|e| async_graphql::Error::new(e.to_string()))?,
                }
            }
            DeploymentKindGql::Failover => {
                let s = input
                    .spare
                    .ok_or_else(|| async_graphql::Error::new("spare required for FAILOVER"))?;
                DeploymentKind::Failover {
                    spare: NodeId::new(s).map_err(|e| async_graphql::Error::new(e.to_string()))?,
                }
            }
        };

        let run = api
            .engine
            .start_deployment(&node_id, kind)
            .await
            .map_err(to_err)?;
        Ok(RunObject::from(&run))
    }
}
