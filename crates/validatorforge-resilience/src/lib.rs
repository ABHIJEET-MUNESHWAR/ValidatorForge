//! Reusable resilience primitives, all generic over an injectable [`Clock`] so
//! they can be tested deterministically without sleeping.
//!
//! Every fallible boundary in ValidatorForge (node-agent RPC, infra apply,
//! catch-up polling) is wrapped with some combination of these:
//!
//! - [`with_timeout`] — bound the wall-clock cost of one attempt.
//! - [`RetryPolicy`] — bounded retries with equal-jitter backoff (no `rand` dep).
//! - [`CircuitBreaker`] — stop hammering a failing dependency.
//! - [`RateLimiter`] — token-bucket admission control.
//! - [`Bulkhead`] — cap in-flight concurrency to isolate failures.

#![forbid(unsafe_code)]

mod breaker;
mod bulkhead;
mod clock;
mod ratelimit;
mod retry;
mod timeout;

pub use breaker::{BreakerError, BreakerState, CircuitBreaker};
pub use bulkhead::{Bulkhead, BulkheadGuard};
pub use clock::{Clock, ManualClock, SystemClock};
pub use ratelimit::RateLimiter;
pub use retry::RetryPolicy;
pub use timeout::{with_timeout, TimeoutError};
