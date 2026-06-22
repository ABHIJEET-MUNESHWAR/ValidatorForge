//! Durable Postgres repository (feature `postgres`).
//!
//! Uses **runtime** `sqlx` queries (not the compile-time `query!` macros) so the
//! crate builds without a live database or offline metadata. The `deployment_runs`
//! table is **range-partitioned by `started_at`** (monthly partitions + a default
//! catch-all), satisfying the partitioning/sharding requirement: run history is
//! the high-volume, time-series table and partition pruning keeps recent-window
//! queries fast while old partitions can be detached/archived cheaply.

use async_trait::async_trait;
use sqlx::{PgPool, Row};
use validatorforge_core::{NodeRepository, PortError};
use validatorforge_types::{DeploymentRun, NodeId, RunId, ValidatorNode};

fn to_port(e: sqlx::Error) -> PortError {
    match e {
        sqlx::Error::PoolTimedOut => PortError::Timeout("db pool".into()),
        sqlx::Error::Io(_) | sqlx::Error::PoolClosed => {
            PortError::Unavailable("database unavailable".into())
        }
        other => PortError::Internal(other.to_string()),
    }
}

fn decode<T: serde::de::DeserializeOwned>(v: serde_json::Value) -> Result<T, PortError> {
    serde_json::from_value(v).map_err(|e| PortError::Internal(format!("decode: {e}")))
}

/// A Postgres-backed [`NodeRepository`].
#[derive(Clone)]
pub struct PgNodeRepository {
    pool: PgPool,
}

impl PgNodeRepository {
    /// Wrap an existing connection pool.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Apply the embedded migrations (schema + partitions).
    ///
    /// # Errors
    /// Propagates migration failures.
    pub async fn run_migrations(&self) -> Result<(), PortError> {
        sqlx::migrate!("./migrations")
            .run(&self.pool)
            .await
            .map_err(|e| PortError::Internal(e.to_string()))
    }
}

#[async_trait]
impl NodeRepository for PgNodeRepository {
    async fn save_node(&self, node: &ValidatorNode) -> Result<(), PortError> {
        let payload =
            serde_json::to_value(node).map_err(|e| PortError::Internal(format!("encode: {e}")))?;
        sqlx::query(
            "INSERT INTO nodes (id, payload, updated_at) VALUES ($1, $2, $3)
             ON CONFLICT (id) DO UPDATE SET payload = EXCLUDED.payload, updated_at = EXCLUDED.updated_at",
        )
        .bind(node.id().to_string())
        .bind(&payload)
        .bind(node.updated_at())
        .execute(&self.pool)
        .await
        .map_err(to_port)?;
        Ok(())
    }

    async fn get_node(&self, id: &NodeId) -> Result<Option<ValidatorNode>, PortError> {
        let row = sqlx::query("SELECT payload FROM nodes WHERE id = $1")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(to_port)?;
        match row {
            Some(r) => {
                let v: serde_json::Value = r.try_get("payload").map_err(to_port)?;
                Ok(Some(decode(v)?))
            }
            None => Ok(None),
        }
    }

    async fn list_nodes(&self) -> Result<Vec<ValidatorNode>, PortError> {
        let rows = sqlx::query("SELECT payload FROM nodes ORDER BY id")
            .fetch_all(&self.pool)
            .await
            .map_err(to_port)?;
        rows.into_iter()
            .map(|r| {
                let v: serde_json::Value = r.try_get("payload").map_err(to_port)?;
                decode(v)
            })
            .collect()
    }

    async fn save_run(&self, run: &DeploymentRun) -> Result<(), PortError> {
        let payload =
            serde_json::to_value(run).map_err(|e| PortError::Internal(format!("encode: {e}")))?;
        sqlx::query(
            "INSERT INTO deployment_runs (id, target, status, started_at, payload)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (id, started_at)
             DO UPDATE SET status = EXCLUDED.status, payload = EXCLUDED.payload",
        )
        .bind(run.id().to_string())
        .bind(run.target().to_string())
        .bind(run.status().as_str())
        .bind(run.started_at())
        .bind(&payload)
        .execute(&self.pool)
        .await
        .map_err(to_port)?;
        Ok(())
    }

    async fn get_run(&self, id: RunId) -> Result<Option<DeploymentRun>, PortError> {
        let row = sqlx::query("SELECT payload FROM deployment_runs WHERE id = $1")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(to_port)?;
        match row {
            Some(r) => {
                let v: serde_json::Value = r.try_get("payload").map_err(to_port)?;
                Ok(Some(decode(v)?))
            }
            None => Ok(None),
        }
    }

    async fn list_runs(&self, limit: usize) -> Result<Vec<DeploymentRun>, PortError> {
        let rows =
            sqlx::query("SELECT payload FROM deployment_runs ORDER BY started_at DESC LIMIT $1")
                .bind(i64::try_from(limit).unwrap_or(i64::MAX))
                .fetch_all(&self.pool)
                .await
                .map_err(to_port)?;
        rows.into_iter()
            .map(|r| {
                let v: serde_json::Value = r.try_get("payload").map_err(to_port)?;
                decode(v)
            })
            .collect()
    }
}
