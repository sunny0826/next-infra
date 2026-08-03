//! Versioned query data-transfer objects shared by Desktop and MCP adapters.

use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    pub scope: String,
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

/// Evidence details needed by the shared inspector without exposing provider payloads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[cfg_attr(feature = "typescript-bindings", derive(ts_rs::TS))]
pub enum RelationEvidenceDto {
    Provider {
        connector_type: String,
        connection_id: String,
        sync_run_id: String,
        field_path: String,
    },
    Configured {
        binding_id: String,
        created_at: String,
    },
    Inferred {
        rule_version: String,
        input_resource_version_ids: Vec<String>,
        input_relation_version_ids: Vec<String>,
        confidence_basis_points: u16,
    },
}

/// Minimal current relation projection for query consumers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript-bindings", derive(ts_rs::TS))]
pub struct RelationDto {
    pub relation_id: String,
    pub source_resource_id: String,
    pub target_resource_id: String,
    pub kind: String,
    pub lifecycle: Lifecycle,
    pub evidence_type: EvidenceType,
    pub evidence: RelationEvidenceDto,
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

/// UI-visible lifecycle of a bounded query request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "typescript-bindings", derive(ts_rs::TS))]
pub enum QueryViewState {
    Loading,
    Ready,
    Empty,
    Partial,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "typescript-bindings", derive(ts_rs::TS))]
pub enum SyncModeDto {
    Full,
    Incremental,
    Targeted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "typescript-bindings", derive(ts_rs::TS))]
pub enum SyncTriggerDto {
    Schedule,
    User,
    Startup,
    Recovery,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "typescript-bindings", derive(ts_rs::TS))]
pub enum SyncRunStatusDto {
    Running,
    Succeeded,
    Partial,
    Failed,
    Cancelled,
    Interrupted,
}

/// Coverage reported by a single synchronization run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[cfg_attr(feature = "typescript-bindings", derive(ts_rs::TS))]
pub enum SyncCoverageDto {
    AuthoritativeFull {
        scope: String,
    },
    Incremental {
        cursor: String,
    },
    Partial {
        scope: Option<String>,
        reason: String,
    },
    Targeted {
        resource_ids: Vec<String>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "typescript-bindings", derive(ts_rs::TS))]
pub enum ConnectorCoverageLevelDto {
    Supported,
    Partial,
    Unsupported,
}

/// Product-module support declared by a connector descriptor.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript-bindings", derive(ts_rs::TS))]
pub struct ConnectorCoverageDto {
    pub connector_type: String,
    pub connector_version: String,
    pub module: String,
    pub level: ConnectorCoverageLevelDto,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript-bindings", derive(ts_rs::TS))]
pub struct SyncRunCountsDto {
    #[cfg_attr(feature = "typescript-bindings", ts(type = "number"))]
    pub read: u64,
    #[cfg_attr(feature = "typescript-bindings", ts(type = "number"))]
    pub created: u64,
    #[cfg_attr(feature = "typescript-bindings", ts(type = "number"))]
    pub updated: u64,
    #[cfg_attr(feature = "typescript-bindings", ts(type = "number"))]
    pub unchanged: u64,
    #[cfg_attr(feature = "typescript-bindings", ts(type = "number"))]
    pub warnings: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript-bindings", derive(ts_rs::TS))]
pub struct SyncRunErrorDto {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

/// User-safe synchronization history row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript-bindings", derive(ts_rs::TS))]
pub struct SyncRunDto {
    pub sync_run_id: String,
    pub connection_id: String,
    pub mode: SyncModeDto,
    pub trigger: SyncTriggerDto,
    pub status: SyncRunStatusDto,
    pub coverage: SyncCoverageDto,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub cursor_before: Option<String>,
    pub cursor_after: Option<String>,
    pub counts: SyncRunCountsDto,
    pub errors: Vec<SyncRunErrorDto>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[cfg_attr(feature = "typescript-bindings", derive(ts_rs::TS))]
pub enum ChangeSubjectDto {
    Resource { resource_id: String },
    Relation { relation_id: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript-bindings", derive(ts_rs::TS))]
pub struct FieldChangeDto {
    pub path: String,
    #[cfg_attr(feature = "typescript-bindings", ts(type = "unknown | null"))]
    pub before: Option<Value>,
    #[cfg_attr(feature = "typescript-bindings", ts(type = "unknown | null"))]
    pub after: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[cfg_attr(feature = "typescript-bindings", derive(ts_rs::TS))]
pub enum ChangeOriginDto {
    SyncRun {
        sync_run_id: String,
    },
    Binding {
        binding_id: String,
    },
    Inference {
        rule_version: String,
        input_resource_version_ids: Vec<String>,
    },
}

/// Field-level change linked to its persisted evidence origin.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript-bindings", derive(ts_rs::TS))]
pub struct ChangeDto {
    pub change_id: String,
    pub subject: ChangeSubjectDto,
    pub observed_at: String,
    pub fields: Vec<FieldChangeDto>,
    pub origin: ChangeOriginDto,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript-bindings", derive(ts_rs::TS))]
pub struct ResourcePageDto {
    pub metadata: SnapshotMetadata,
    pub items: Vec<ResourceDto>,
    pub page_info: PageInfo,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript-bindings", derive(ts_rs::TS))]
pub struct ResourceDetailDto {
    pub metadata: SnapshotMetadata,
    pub resource: ResourceDto,
    #[cfg_attr(feature = "typescript-bindings", ts(type = "unknown"))]
    pub attributes: Value,
    pub relations: Vec<RelationDto>,
    pub recent_changes: Vec<ChangeDto>,
    pub connector_coverage: Vec<ConnectorCoverageDto>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "typescript-bindings", derive(ts_rs::TS))]
pub enum FrontierDirectionDto {
    Incoming,
    Outgoing,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript-bindings", derive(ts_rs::TS))]
pub struct TopologyFrontierDto {
    pub resource_id: String,
    pub direction: FrontierDirectionDto,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript-bindings", derive(ts_rs::TS))]
pub struct TopologyDto {
    pub metadata: SnapshotMetadata,
    pub focus_resource_id: String,
    pub depth: u8,
    pub nodes: Vec<ResourceDto>,
    pub edges: Vec<RelationDto>,
    pub frontier: Vec<TopologyFrontierDto>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript-bindings", derive(ts_rs::TS))]
pub struct ResourceHealthCountsDto {
    #[cfg_attr(feature = "typescript-bindings", ts(type = "number"))]
    pub healthy: u64,
    #[cfg_attr(feature = "typescript-bindings", ts(type = "number"))]
    pub degraded: u64,
    #[cfg_attr(feature = "typescript-bindings", ts(type = "number"))]
    pub unhealthy: u64,
    #[cfg_attr(feature = "typescript-bindings", ts(type = "number"))]
    pub unknown: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript-bindings", derive(ts_rs::TS))]
pub struct FreshnessCountsDto {
    #[cfg_attr(feature = "typescript-bindings", ts(type = "number"))]
    pub fresh: u64,
    #[cfg_attr(feature = "typescript-bindings", ts(type = "number"))]
    pub stale: u64,
    #[cfg_attr(feature = "typescript-bindings", ts(type = "number"))]
    pub expired: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript-bindings", derive(ts_rs::TS))]
pub struct ConnectorHealthCountsDto {
    #[cfg_attr(feature = "typescript-bindings", ts(type = "number"))]
    pub healthy: u64,
    #[cfg_attr(feature = "typescript-bindings", ts(type = "number"))]
    pub degraded: u64,
    #[cfg_attr(feature = "typescript-bindings", ts(type = "number"))]
    pub auth_failed: u64,
    #[cfg_attr(feature = "typescript-bindings", ts(type = "number"))]
    pub rate_limited: u64,
    #[cfg_attr(feature = "typescript-bindings", ts(type = "number"))]
    pub unreachable: u64,
    #[cfg_attr(feature = "typescript-bindings", ts(type = "number"))]
    pub disabled: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript-bindings", derive(ts_rs::TS))]
pub struct HealthSummaryDto {
    pub metadata: SnapshotMetadata,
    pub resource_health: ResourceHealthCountsDto,
    pub freshness: FreshnessCountsDto,
    pub connector_health: ConnectorHealthCountsDto,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript-bindings", derive(ts_rs::TS))]
pub struct ChangePageDto {
    pub metadata: SnapshotMetadata,
    pub items: Vec<ChangeDto>,
    pub page_info: PageInfo,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript-bindings", derive(ts_rs::TS))]
pub struct SyncStatusDto {
    pub metadata: SnapshotMetadata,
    pub connection: ConnectionDto,
    pub recent_runs: Vec<SyncRunDto>,
    pub next_scheduled_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript-bindings", derive(ts_rs::TS))]
pub struct ConnectorCoverageSnapshotDto {
    pub metadata: SnapshotMetadata,
    pub items: Vec<ConnectorCoverageDto>,
}
