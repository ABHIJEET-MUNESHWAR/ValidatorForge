//! Validated identity and value newtypes.
//!
//! Each newtype validates on construction so that, once you hold one, you can
//! rely on its invariants everywhere downstream — no defensive re-checking.

use serde::{Deserialize, Serialize};

use crate::error::DomainError;

/// Maximum length of a node identifier.
pub const MAX_NODE_ID_LEN: usize = 64;
/// Maximum length of a cluster name.
pub const MAX_CLUSTER_NAME_LEN: usize = 32;
/// Maximum length of a host address.
pub const MAX_HOST_ADDR_LEN: usize = 253;

/// Opaque, validated identifier for a validator node within the fleet.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NodeId(String);

impl NodeId {
    /// Construct a node id, rejecting empty or over-long values.
    ///
    /// # Errors
    /// Returns [`DomainError`] when empty or longer than [`MAX_NODE_ID_LEN`].
    pub fn new(raw: impl Into<String>) -> Result<Self, DomainError> {
        let raw = raw.into();
        if raw.trim().is_empty() {
            return Err(DomainError::Empty { field: "node_id" });
        }
        if raw.len() > MAX_NODE_ID_LEN {
            return Err(DomainError::TooLong {
                field: "node_id",
                max: MAX_NODE_ID_LEN,
                got: raw.len(),
            });
        }
        Ok(Self(raw))
    }

    /// Borrow the inner string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A human-readable cluster / region label (e.g. `"eu-central-fiber"`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ClusterName(String);

impl ClusterName {
    /// Construct a cluster name, rejecting empty or over-long values.
    ///
    /// # Errors
    /// Returns [`DomainError`] when empty or longer than [`MAX_CLUSTER_NAME_LEN`].
    pub fn new(raw: impl Into<String>) -> Result<Self, DomainError> {
        let raw = raw.into();
        if raw.trim().is_empty() {
            return Err(DomainError::Empty {
                field: "cluster_name",
            });
        }
        if raw.len() > MAX_CLUSTER_NAME_LEN {
            return Err(DomainError::TooLong {
                field: "cluster_name",
                max: MAX_CLUSTER_NAME_LEN,
                got: raw.len(),
            });
        }
        Ok(Self(raw))
    }

    /// Borrow the inner string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ClusterName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A validator client version (free-form semver-ish string, e.g. `"2.0.14"`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ValidatorVersion(String);

impl ValidatorVersion {
    /// Construct a version, rejecting empty values.
    ///
    /// # Errors
    /// Returns [`DomainError::Empty`] when empty.
    pub fn new(raw: impl Into<String>) -> Result<Self, DomainError> {
        let raw = raw.into();
        if raw.trim().is_empty() {
            return Err(DomainError::Empty { field: "version" });
        }
        Ok(Self(raw))
    }

    /// Borrow the inner string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ValidatorVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A network address (DNS name or IP) for a node host.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HostAddr(String);

impl HostAddr {
    /// Construct a host address, rejecting empty or over-long values.
    ///
    /// # Errors
    /// Returns [`DomainError`] when empty or longer than [`MAX_HOST_ADDR_LEN`].
    pub fn new(raw: impl Into<String>) -> Result<Self, DomainError> {
        let raw = raw.into();
        if raw.trim().is_empty() {
            return Err(DomainError::Empty { field: "host" });
        }
        if raw.len() > MAX_HOST_ADDR_LEN {
            return Err(DomainError::TooLong {
                field: "host",
                max: MAX_HOST_ADDR_LEN,
                got: raw.len(),
            });
        }
        Ok(Self(raw))
    }

    /// Borrow the inner string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for HostAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A Solana slot number.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize,
)]
pub struct Slot(pub u64);

impl Slot {
    /// The raw slot value.
    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for Slot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Identifier for an orchestration run (deployment / upgrade / failover saga).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RunId(pub u128);

impl RunId {
    /// The raw run value.
    #[must_use]
    pub const fn value(self) -> u128 {
        self.0
    }
}

impl std::fmt::Display for RunId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:032x}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_id_rejects_empty_and_blank() {
        assert_eq!(NodeId::new("").unwrap_err().code(), crate::ErrorCode::Empty);
        assert_eq!(
            NodeId::new("   ").unwrap_err().code(),
            crate::ErrorCode::Empty
        );
    }

    #[test]
    fn node_id_rejects_too_long() {
        let long = "x".repeat(MAX_NODE_ID_LEN + 1);
        assert_eq!(
            NodeId::new(long).unwrap_err().code(),
            crate::ErrorCode::TooLong
        );
    }

    #[test]
    fn node_id_roundtrips_display_and_accessor() {
        let id = NodeId::new("eu-val-01").unwrap();
        assert_eq!(id.as_str(), "eu-val-01");
        assert_eq!(id.to_string(), "eu-val-01");
    }

    #[test]
    fn cluster_name_validates() {
        assert!(ClusterName::new("eu-central-fiber").is_ok());
        assert!(ClusterName::new("").is_err());
        assert!(ClusterName::new("x".repeat(MAX_CLUSTER_NAME_LEN + 1)).is_err());
    }

    #[test]
    fn version_validates_and_orders() {
        let a = ValidatorVersion::new("2.0.9").unwrap();
        let b = ValidatorVersion::new("2.0.14").unwrap();
        // Ordering is lexical by design; callers compare exact strings rather
        // than relying on semver precedence.
        assert!(a > b);
        assert!(ValidatorVersion::new("").is_err());
    }

    #[test]
    fn host_addr_validates() {
        assert!(HostAddr::new("val01.eu.fiber.internal").is_ok());
        assert!(HostAddr::new("").is_err());
        assert!(HostAddr::new("x".repeat(MAX_HOST_ADDR_LEN + 1)).is_err());
    }

    #[test]
    fn slot_and_run_id_display() {
        assert_eq!(Slot(42).value(), 42);
        assert_eq!(Slot(7).to_string(), "7");
        let run = RunId(0xabc);
        assert_eq!(run.value(), 0xabc);
        assert_eq!(run.to_string().len(), 32);
    }

    #[test]
    fn ids_serde_roundtrip() {
        let id = NodeId::new("n1").unwrap();
        let json = serde_json::to_string(&id).unwrap();
        let back: NodeId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }
}
