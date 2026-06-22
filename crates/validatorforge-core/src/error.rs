//! Core error taxonomy.

use thiserror::Error;
use validatorforge_types::DomainError;

/// Failures returned by outbound ports (adapters).
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum PortError {
    /// The dependency is temporarily unavailable (retryable).
    #[error("dependency unavailable: {0}")]
    Unavailable(String),
    /// The operation exceeded its deadline (retryable).
    #[error("operation timed out: {0}")]
    Timeout(String),
    /// The dependency rejected the request (not retryable).
    #[error("rejected: {0}")]
    Rejected(String),
    /// An unexpected internal adapter error (not retryable).
    #[error("internal error: {0}")]
    Internal(String),
}

impl PortError {
    /// Whether a retry could plausibly succeed.
    #[must_use]
    pub const fn is_retryable(&self) -> bool {
        matches!(self, PortError::Unavailable(_) | PortError::Timeout(_))
    }

    /// Stable wire code.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            PortError::Unavailable(_) => "UNAVAILABLE",
            PortError::Timeout(_) => "TIMEOUT",
            PortError::Rejected(_) => "REJECTED",
            PortError::Internal(_) => "INTERNAL",
        }
    }
}

/// Errors surfaced by the application core to its callers (the API layer).
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CoreError {
    /// A domain invariant was violated (bad input or illegal transition).
    #[error(transparent)]
    Domain(#[from] DomainError),
    /// An outbound port failed.
    #[error(transparent)]
    Port(#[from] PortError),
    /// The requested entity does not exist.
    #[error("not found: {0}")]
    NotFound(String),
    /// A request was throttled by the admission rate limiter.
    #[error("request throttled")]
    Throttled,
    /// The circuit breaker is open for the targeted dependency.
    #[error("circuit open")]
    CircuitOpen,
}

impl CoreError {
    /// Stable wire code surfaced to GraphQL `extensions.code`.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            CoreError::Domain(e) => e.code().as_str(),
            CoreError::Port(e) => e.code(),
            CoreError::NotFound(_) => "NOT_FOUND",
            CoreError::Throttled => "THROTTLED",
            CoreError::CircuitOpen => "CIRCUIT_OPEN",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retryable_classification() {
        assert!(PortError::Unavailable("x".into()).is_retryable());
        assert!(PortError::Timeout("x".into()).is_retryable());
        assert!(!PortError::Rejected("x".into()).is_retryable());
        assert!(!PortError::Internal("x".into()).is_retryable());
    }

    #[test]
    fn core_error_codes() {
        assert_eq!(
            CoreError::Port(PortError::Unavailable("x".into())).code(),
            "UNAVAILABLE"
        );
        assert_eq!(CoreError::NotFound("n".into()).code(), "NOT_FOUND");
        assert_eq!(CoreError::Throttled.code(), "THROTTLED");
        assert_eq!(CoreError::CircuitOpen.code(), "CIRCUIT_OPEN");
    }

    #[test]
    fn port_codes() {
        assert_eq!(PortError::Rejected("x".into()).code(), "REJECTED");
        assert_eq!(PortError::Internal("x".into()).code(), "INTERNAL");
    }
}
