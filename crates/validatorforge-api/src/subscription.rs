//! Subscription root — the live read side over the ops event stream.

use async_graphql::{Context, Subscription};
use futures::{Stream, StreamExt};

use crate::schema::ApiContext;
use crate::types::OpsEventObject;

/// Streaming entry points.
pub struct SubscriptionRoot;

#[Subscription]
impl SubscriptionRoot {
    /// Live stream of ops events (run/step/node-state lifecycle).
    ///
    /// Each subscriber gets an independent broadcast stream; a slow consumer
    /// only drops its own backlog and never stalls the engine.
    async fn ops_events(&self, ctx: &Context<'_>) -> impl Stream<Item = OpsEventObject> + 'static {
        ctx.data_unchecked::<ApiContext>()
            .events
            .subscribe()
            .map(OpsEventObject::from)
    }
}
