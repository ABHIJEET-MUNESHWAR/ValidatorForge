//! `validatorforge-api` — the GraphQL surface (GraphQL over REST).
//!
//! Defines the schema (queries, mutations, and a live subscription) plus the
//! [`ApiContext`] the composition root injects. Transport (axum + WebSocket)
//! lives in `validatorforge-node`; this crate is transport-agnostic so the
//! schema can be unit-tested in-process against the simulated infra adapters.

#![forbid(unsafe_code)]

pub mod mutation;
pub mod query;
pub mod schema;
pub mod subscription;
pub mod types;

pub use mutation::{MutationRoot, RegisterNodeInput, StartDeploymentInput};
pub use query::QueryRoot;
pub use schema::{build_schema, ApiContext, ValidatorForgeSchema};
pub use subscription::SubscriptionRoot;

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use validatorforge_ai::HeuristicAdvisor;
    use validatorforge_core::{
        Clock, EventSink, EventStream, NodeRepository, OpsConfig, OpsEngine,
    };
    use validatorforge_infra::{
        BroadcastEventSink, InMemoryNodeRepository, SimNodeAgent, UtcClock,
    };

    use super::*;

    fn test_schema() -> ValidatorForgeSchema {
        let repo: Arc<dyn NodeRepository> = Arc::new(InMemoryNodeRepository::new());
        let agent = Arc::new(SimNodeAgent::new());
        let sink = Arc::new(BroadcastEventSink::new(1024));
        let advisor = Arc::new(HeuristicAdvisor::new());
        let clock: Arc<dyn Clock> = Arc::new(UtcClock::new());
        let cfg = OpsConfig {
            op_timeout: std::time::Duration::from_millis(200),
            retry_base: std::time::Duration::from_millis(1),
            retry_max: std::time::Duration::from_millis(2),
            ..OpsConfig::default()
        };
        let engine = OpsEngine::new(
            repo,
            agent,
            sink.clone() as Arc<dyn EventSink>,
            advisor,
            clock,
            cfg,
        );
        let ctx = ApiContext::new(Arc::new(engine), sink as Arc<dyn EventStream>);
        build_schema(ctx)
    }

    const REGISTER: &str = r#"mutation {
        registerNode(input: {
            id: "eu-val-01", clusterName: "eu-fiber", cluster: TESTNET,
            host: "val01.internal", role: VOTING, version: "2.0.14"
        }) { id state }
    }"#;

    #[tokio::test]
    async fn health_reports_ok() {
        let schema = test_schema();
        let res = schema.execute("{ health { status nodes } }").await;
        assert!(res.errors.is_empty(), "{:?}", res.errors);
        let data = res.data.into_json().unwrap();
        assert_eq!(data["health"]["status"], "ok");
        assert_eq!(data["health"]["nodes"], 0);
    }

    #[tokio::test]
    async fn register_then_list_node() {
        let schema = test_schema();
        let res = schema.execute(REGISTER).await;
        assert!(res.errors.is_empty(), "{:?}", res.errors);
        let data = res.data.into_json().unwrap();
        assert_eq!(data["registerNode"]["state"], "provisioning");

        let res = schema.execute("{ nodes { id state } }").await;
        let data = res.data.into_json().unwrap();
        assert_eq!(data["nodes"][0]["id"], "eu-val-01");
    }

    #[tokio::test]
    async fn provision_deployment_succeeds_and_activates() {
        let schema = test_schema();
        assert!(schema.execute(REGISTER).await.errors.is_empty());

        let res = schema
            .execute(
                r#"mutation {
                    startDeployment(input: { nodeId: "eu-val-01", kind: PROVISION }) {
                        status completedSteps
                    }
                }"#,
            )
            .await;
        assert!(res.errors.is_empty(), "{:?}", res.errors);
        let data = res.data.into_json().unwrap();
        assert_eq!(data["startDeployment"]["status"], "succeeded");
        assert_eq!(
            data["startDeployment"]["completedSteps"]
                .as_array()
                .unwrap()
                .len(),
            4
        );

        // Node should now be active.
        let res = schema
            .execute(r#"{ node(id: "eu-val-01") { state } }"#)
            .await;
        let data = res.data.into_json().unwrap();
        assert_eq!(data["node"]["state"], "active");
    }

    #[tokio::test]
    async fn upgrade_without_target_version_errors() {
        let schema = test_schema();
        assert!(schema.execute(REGISTER).await.errors.is_empty());
        let res = schema
            .execute(
                r#"mutation {
                    startDeployment(input: { nodeId: "eu-val-01", kind: UPGRADE }) { status }
                }"#,
            )
            .await;
        assert!(!res.errors.is_empty());
    }

    #[tokio::test]
    async fn advise_returns_recommendation() {
        let schema = test_schema();
        assert!(schema.execute(REGISTER).await.errors.is_empty());
        let res = schema
            .execute(r#"{ advise(nodeId: "eu-val-01", context: "all good") }"#)
            .await;
        assert!(res.errors.is_empty(), "{:?}", res.errors);
        let data = res.data.into_json().unwrap();
        assert!(data["advise"].as_str().unwrap().contains("action"));
    }

    #[tokio::test]
    async fn unknown_node_query_is_null() {
        let schema = test_schema();
        let res = schema.execute(r#"{ node(id: "ghost") { id } }"#).await;
        assert!(res.errors.is_empty(), "{:?}", res.errors);
        let data = res.data.into_json().unwrap();
        assert!(data["node"].is_null());
    }
}
