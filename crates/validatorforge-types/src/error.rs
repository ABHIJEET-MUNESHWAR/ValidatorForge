//! Domain error type shared across the type layer.

use thiserror::Error;

/// Stable, machine-readable error codes surfaced all the way to GraphQL
/// `extensions.code`. Keeping them in one enum means the wire contract is
/// defined in exactly one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    /// A node identity / value failed validation on construction.
    InvalidNode,
    /// A field was empty when a non-empty value was required.
    Empty,
    /// A string field exceeded its maximum length.
    TooLong,
    /// A numeric field was outside its permitted range.
    OutOfRange,
    /// A lifecycle transition was not permitted by the state machine.
    IllegalTransition,
}

impl ErrorCode {
    /// The stable string code used on the wire.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ErrorCode::InvalidNode => "INVALID_NODE",
            ErrorCode::Empty => "EMPTY",
            ErrorCode::TooLong => "TOO_LONG",
            ErrorCode::OutOfRange => "OUT_OF_RANGE",
            ErrorCode::IllegalTransition => "ILLEGAL_TRANSITION",
        }
    }
}

/// Errors produced while constructing or transitioning domain values.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum DomainError {
    /// A required string field was empty.
    #[error("{field} must not be empty")]
    Empty {
        /// The offending field name.
        field: &'static str,
    },
    /// A string field was longer than allowed.
    #[error("{field} must be at most {max} characters, got {got}")]
    TooLong {
        /// The offending field name.
        field: &'static str,
        /// The configured maximum length.
        max: usize,
        /// The actual length supplied.
        got: usize,
    },
    /// A numeric field was outside its permitted range.
    #[error("{field} must be in [{min}, {max}], got {got}")]
    OutOfRange {
        /// The offending field name.
        field: &'static str,
        /// Inclusive lower bound.
        min: i64,
        /// Inclusive upper bound.
        max: i64,
        /// The actual value supplied.
        got: i64,
    },
    /// A lifecycle transition violated the state machine.
    #[error("illegal transition from {from} to {to}")]
    IllegalTransition {
        /// The current state name.
        from: &'static str,
        /// The attempted target state name.
        to: &'static str,
    },
}

impl DomainError {
    /// Map the error to its stable [`ErrorCode`].
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            DomainError::Empty { .. } => ErrorCode::Empty,
            DomainError::TooLong { .. } => ErrorCode::TooLong,
            DomainError::OutOfRange { .. } => ErrorCode::OutOfRange,
            DomainError::IllegalTransition { .. } => ErrorCode::IllegalTransition,
        }
    }
}
