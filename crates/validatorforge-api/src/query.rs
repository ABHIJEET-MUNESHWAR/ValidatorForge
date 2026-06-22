//! Query root — the read side.

use async_graphql::{Context, Object, Result};
use validatorforge_types::{NodeId, RunId};

use crate::schema::{to_err, ApiContext};
use crate::types::{HealthObject, NodeObject, RunObject};

/// Read entry points.
pub struct QueryRoot;

#[Object]
impl QueryRoot {
    /// Service health: status + number of tracked nodes.
    async fn health(&self, ctx: &Context<'_>) -> Result<HealthObject> {
        let api = ctx.data_unchecked::<ApiContext>();
        let nodes = api.engine.list_nodes().await.map_err(to_err)?;
        Ok(HealthObject {
            status: "ok".to_string(),
            nodes: i32::try_from(nodes.len()).unwrap_or(i32::MAX),
        })
    }

    /// Fetch a single node by id.
    async fn node(&self, ctx: &Context<'_>, id: String) -> Result<Option<NodeObject>> {
        let api = ctx.data_unchecked::<ApiContext>();
        let id = NodeId::new(id).map_err(|e| async_graphql::Error::new(e.to_string()))?;
        let node = api.engine.get_node(&id).await.map_err(to_err)?;
        Ok(node.as_ref().map(NodeObject::from))
    }

    /// List all nodes in the fleet.
    async fn nodes(&self, ctx: &Context<'_>) -> Result<Vec<NodeObject>> {
        let api = ctx.data_unchecked::<ApiContext>();
        let nodes = api.engine.list_nodes().await.map_err(to_err)?;
        Ok(nodes.iter().map(NodeObject::from).collect())
    }

    /// Fetch a single deployment run by id.
    async fn run(&self, ctx: &Context<'_>, id: String) -> Result<Option<RunObject>> {
        let api = ctx.data_unchecked::<ApiContext>();
        let run_id = id
            .parse::<u128>()
            .map(RunId)
            .map_err(|_| async_graphql::Error::new("invalid run id"))?;
        let run = api.engine.get_run(run_id).await.map_err(to_err)?;
        Ok(run.as_ref().map(RunObject::from))
    }

    /// List recent runs (newest first), capped at `limit`.
    async fn runs(
        &self,
        ctx: &Context<'_>,
        #[graphql(default = 20)] limit: i32,
    ) -> Result<Vec<RunObject>> {
        let api = ctx.data_unchecked::<ApiContext>();
        let limit = usize::try_from(limit.max(0)).unwrap_or(0);
        let runs = api.engine.list_runs(limit).await.map_err(to_err)?;
        Ok(runs.iter().map(RunObject::from).collect())
    }

    /// Ask the AI advisor for a recommendation about a node.
    async fn advise(
        &self,
        ctx: &Context<'_>,
        node_id: String,
        #[graphql(default)] context: String,
    ) -> Result<String> {
        let api = ctx.data_unchecked::<ApiContext>();
        let id = NodeId::new(node_id).map_err(|e| async_graphql::Error::new(e.to_string()))?;
        api.engine.advise(&id, &context).await.map_err(to_err)
    }
}
