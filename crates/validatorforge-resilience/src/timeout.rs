//! Single-attempt timeout wrapper.

use std::future::Future;
use std::time::Duration;

use thiserror::Error;

/// Returned when a future does not complete within its deadline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("operation timed out after {0:?}")]
pub struct TimeoutError(pub Duration);

/// Run `fut`, failing with [`TimeoutError`] if it exceeds `dur`.
///
/// # Errors
/// Returns [`TimeoutError`] when the deadline elapses before `fut` resolves.
pub async fn with_timeout<F, T>(dur: Duration, fut: F) -> Result<T, TimeoutError>
where
    F: Future<Output = T>,
{
    tokio::time::timeout(dur, fut)
        .await
        .map_err(|_| TimeoutError(dur))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn completes_within_deadline() {
        let v = with_timeout(Duration::from_secs(1), async { 7 })
            .await
            .unwrap();
        assert_eq!(v, 7);
    }

    #[tokio::test(start_paused = true)]
    async fn times_out() {
        let fut = with_timeout(Duration::from_millis(10), async {
            tokio::time::sleep(Duration::from_secs(60)).await;
            1
        });
        let err = fut.await.unwrap_err();
        assert_eq!(err, TimeoutError(Duration::from_millis(10)));
    }
}
