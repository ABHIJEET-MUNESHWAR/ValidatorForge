//! Schema assembly: the shared context, an error helper, and the
//! depth/complexity-limited schema.

use std::sync::Arc;

use async_graphql::{ErrorExtensions, Schema};
use validatorforge_core::{CoreError, EventStream, OpsEngine};

use crate::mutation::MutationRoot;
use crate::query::QueryRoot;
use crate::subscription::SubscriptionRoot;

/// Shared state injected into every resolver via [`async_graphql::Context`].
#[derive(Clone)]
pub struct ApiContext {
    /// The ops engine (orchestration + read model).
    pub engine: Arc<OpsEngine>,
    /// Subscribable event source for the `opsEvents` subscription.
    pub events: Arc<dyn EventStream>,
}

impl ApiContext {
    /// Construct the context.
    #[must_use]
    pub fn new(engine: Arc<OpsEngine>, events: Arc<dyn EventStream>) -> Self {
        Self { engine, events }
    }
}

/// Map a [`CoreError`] into a GraphQL error carrying a stable `code` extension.
pub(crate) fn to_err(e: CoreError) -> async_graphql::Error {
    let code = e.code();
    async_graphql::Error::new(e.to_string()).extend_with(|_, ext| ext.set("code", code))
}

/// The fully-typed schema for this service.
pub type ValidatorForgeSchema = Schema<QueryRoot, MutationRoot, SubscriptionRoot>;

/// Build the schema with the given context.
///
/// Depth and complexity limits cap the cost of any single query — a cheap,
/// always-on guard against pathological/abusive documents (DoS resilience).
#[must_use]
pub fn build_schema(context: ApiContext) -> ValidatorForgeSchema {
    Schema::build(QueryRoot, MutationRoot, SubscriptionRoot)
        .data(context)
        .limit_depth(12)
        .limit_complexity(256)
        .finish()
}
