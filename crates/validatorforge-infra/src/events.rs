//! Broadcast event sink: fans [`OpsEvent`]s out to any number of subscribers
//! (the GraphQL subscription resolver, metrics bridges, future outbox workers).

use async_trait::async_trait;
use futures::StreamExt;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use validatorforge_core::{EventSink, EventStream, OpsEvent};

/// An [`EventSink`] backed by a Tokio broadcast channel.
///
/// Publishing is non-blocking and lossy under slow consumers (a lagging
/// subscriber receives a `Lagged` error on its own receiver, never blocking the
/// engine). This keeps the control-plane hot path free of backpressure from
/// observers.
#[derive(Clone)]
pub struct BroadcastEventSink {
    tx: broadcast::Sender<OpsEvent>,
}

impl BroadcastEventSink {
    /// Create a sink with the given per-subscriber buffer `capacity`.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let (tx, _rx) = broadcast::channel(capacity.max(1));
        Self { tx }
    }

    /// Subscribe to the raw broadcast receiver (lower-level than the
    /// [`validatorforge_core::EventStream`] stream API).
    #[must_use]
    pub fn subscribe_raw(&self) -> broadcast::Receiver<OpsEvent> {
        self.tx.subscribe()
    }

    /// Current number of active subscribers.
    #[must_use]
    pub fn receiver_count(&self) -> usize {
        self.tx.receiver_count()
    }
}

impl Default for BroadcastEventSink {
    fn default() -> Self {
        Self::new(1024)
    }
}

#[async_trait]
impl EventSink for BroadcastEventSink {
    async fn publish(&self, event: OpsEvent) {
        // A send error only means there are no subscribers; that is fine.
        let _ = self.tx.send(event);
    }
}

impl EventStream for BroadcastEventSink {
    fn subscribe(&self) -> futures::stream::BoxStream<'static, OpsEvent> {
        // Drop lag/closed errors: a slow subscriber loses its own backlog only.
        BroadcastStream::new(self.tx.subscribe())
            .filter_map(|r| async move { r.ok() })
            .boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use validatorforge_types::{RunId, RunStatus};

    #[tokio::test]
    async fn delivers_to_subscriber() {
        let sink = BroadcastEventSink::new(8);
        let mut rx = sink.subscribe_raw();
        assert_eq!(sink.receiver_count(), 1);
        let evt = OpsEvent::RunFinished {
            run: RunId(7),
            status: RunStatus::Succeeded,
            at: Utc::now(),
        };
        sink.publish(evt.clone()).await;
        let got = rx.recv().await.unwrap();
        assert_eq!(got, evt);
    }

    #[tokio::test]
    async fn publish_without_subscribers_is_ok() {
        let sink = BroadcastEventSink::new(8);
        sink.publish(OpsEvent::RunFinished {
            run: RunId(1),
            status: RunStatus::Failed,
            at: Utc::now(),
        })
        .await;
        assert_eq!(sink.receiver_count(), 0);
    }
}
