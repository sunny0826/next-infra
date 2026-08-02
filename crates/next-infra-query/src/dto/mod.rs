//! Versioned query data-transfer objects shared by Desktop and MCP adapters.

use serde::{Deserialize, Serialize};

/// Compatibility version for the query DTO contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
#[cfg_attr(feature = "typescript-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript-bindings", ts(type = "number"))]
pub struct SchemaVersion(u32);

impl SchemaVersion {
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

pub const QUERY_DTO_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1);

/// Metadata shared by every committed query snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript-bindings", derive(ts_rs::TS))]
pub struct SnapshotMetadata {
    pub schema_version: SchemaVersion,
    pub snapshot_version: String,
    pub generated_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
struct OpaqueCursor(String);

/// Boundary information for a bounded query page.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript-bindings", derive(ts_rs::TS))]
pub struct PageInfo {
    #[cfg_attr(
        feature = "typescript-bindings",
        ts(type = "(string & { readonly __opaqueCursor: unique symbol }) | null")
    )]
    next_cursor: Option<OpaqueCursor>,
}

impl PageInfo {
    pub fn new(next_cursor: Option<String>) -> Self {
        Self {
            next_cursor: next_cursor.map(OpaqueCursor),
        }
    }

    pub fn next_cursor(&self) -> Option<&str> {
        self.next_cursor.as_ref().map(|cursor| cursor.0.as_str())
    }
}

/// Versioned, user-safe query failure returned instead of an empty result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript-bindings", derive(ts_rs::TS))]
pub struct ErrorEnvelope {
    pub schema_version: SchemaVersion,
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

/// Whether the local resource projection is currently considered present.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "typescript-bindings", derive(ts_rs::TS))]
pub enum Lifecycle {
    Active,
    Tombstoned,
    Orphaned,
}

/// Health reported by a resource at its last observation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "typescript-bindings", derive(ts_rs::TS))]
pub enum ResourceHealth {
    Healthy,
    Degraded,
    Unhealthy,
    Unknown,
}

/// Whether the saved resource observation is current enough for use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "typescript-bindings", derive(ts_rs::TS))]
pub enum Freshness {
    Fresh,
    Stale,
    Expired,
}

/// Minimal current resource projection for query consumers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript-bindings", derive(ts_rs::TS))]
pub struct ResourceDto {
    pub resource_id: String,
    pub connection_id: String,
    pub kind: String,
    pub display_name: String,
    pub lifecycle: Lifecycle,
    pub health: ResourceHealth,
    pub freshness: Freshness,
    pub observed_at: String,
}

/// Provenance class for a resource relation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "typescript-bindings", derive(ts_rs::TS))]
pub enum EvidenceType {
    Provider,
    Configured,
    Inferred,
}

/// Minimal current relation projection for query consumers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript-bindings", derive(ts_rs::TS))]
pub struct RelationDto {
    pub relation_id: String,
    pub source_resource_id: String,
    pub target_resource_id: String,
    pub kind: String,
    pub evidence_type: EvidenceType,
    pub last_seen_at: String,
}

/// Whether a connection can currently read its provider.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "typescript-bindings", derive(ts_rs::TS))]
pub enum ConnectorHealth {
    Healthy,
    Degraded,
    AuthFailed,
    RateLimited,
    Unreachable,
    Disabled,
}

/// Minimal connection status projection for query consumers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript-bindings", derive(ts_rs::TS))]
pub struct ConnectionDto {
    pub connection_id: String,
    pub connector_type: String,
    pub display_name: String,
    pub enabled: bool,
    pub health: ConnectorHealth,
    pub last_success_at: Option<String>,
    pub last_attempt_at: Option<String>,
}
