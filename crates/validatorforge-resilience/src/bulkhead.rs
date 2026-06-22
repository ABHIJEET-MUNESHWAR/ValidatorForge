//! Bulkhead: cap the number of concurrent in-flight operations.

use std::sync::Arc;

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// A concurrency limiter backed by a semaphore. Holding a [`BulkheadGuard`]
/// counts as one in-flight slot; dropping it releases the slot.
#[derive(Debug, Clone)]
pub struct Bulkhead {
    sem: Arc<Semaphore>,
    capacity: usize,
}

/// RAII guard representing one occupied bulkhead slot.
#[derive(Debug)]
pub struct BulkheadGuard {
    _permit: OwnedSemaphorePermit,
}

impl Bulkhead {
    /// Create a bulkhead allowing `capacity` concurrent operations.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            sem: Arc::new(Semaphore::new(capacity)),
            capacity,
        }
    }

    /// Try to claim a slot without waiting; `None` if full.
    pub fn try_acquire(&self) -> Option<BulkheadGuard> {
        self.sem
            .clone()
            .try_acquire_owned()
            .ok()
            .map(|permit| BulkheadGuard { _permit: permit })
    }

    /// Claim a slot, awaiting until one is free.
    ///
    /// # Panics
    /// Never panics in practice; the semaphore is never closed.
    pub async fn acquire(&self) -> BulkheadGuard {
        let permit = self
            .sem
            .clone()
            .acquire_owned()
            .await
            .expect("bulkhead semaphore is never closed");
        BulkheadGuard { _permit: permit }
    }

    /// Slots currently available.
    #[must_use]
    pub fn available(&self) -> usize {
        self.sem.available_permits()
    }

    /// Total configured capacity.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn limits_concurrency() {
        let b = Bulkhead::new(2);
        let g1 = b.try_acquire();
        let g2 = b.try_acquire();
        assert!(g1.is_some());
        assert!(g2.is_some());
        assert!(b.try_acquire().is_none());
        assert_eq!(b.available(), 0);
        drop(g1);
        assert_eq!(b.available(), 1);
        assert!(b.try_acquire().is_some());
    }

    #[tokio::test]
    async fn acquire_awaits_a_slot() {
        let b = Bulkhead::new(1);
        let g = b.acquire().await;
        assert_eq!(b.available(), 0);
        drop(g);
        let _g2 = b.acquire().await;
        assert_eq!(b.capacity(), 1);
    }
}
