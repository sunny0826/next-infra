use crate::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ResourceKey {
    pub connection_id: ConnectionId,
    pub kind: ResourceKind,
    pub external_id: ExternalId,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RelationKey {
    pub source_resource_id: ResourceId,
    pub target_resource_id: ResourceId,
    pub kind: RelationKind,
    pub evidence_type: EvidenceType,
    pub evidence_key: EvidenceKey,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connection {
    pub connection_id: ConnectionId,
    pub connector_type: ConnectorType,
    pub display_name: String,
    pub enabled: bool,
    pub config: Value,
    pub secret_ref: Option<SecretRef>,
    pub health: ConnectorHealth,
    pub last_success_at: Option<Timestamp>,
    pub last_attempt_at: Option<Timestamp>,
    pub config_schema_version: SchemaVersion,
    pub deleted_at: Option<Timestamp>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resource {
    pub resource_id: ResourceId,
    pub connection_id: ConnectionId,
    pub kind: ResourceKind,
    pub external_id: ExternalId,
    pub name: String,
    pub display_name: String,
    pub scope: Scope,
    pub labels: BTreeMap<LabelKey, String>,
    pub lifecycle: Lifecycle,
    pub health: ResourceHealth,
    pub attributes: Value,
    pub attribute_schema_version: SchemaVersion,
    pub fingerprint: Fingerprint,
    pub first_seen_at: Timestamp,
    pub last_seen_at: Timestamp,
    pub last_changed_at: Timestamp,
    pub last_sync_run_id: SyncRunId,
}

impl Resource {
    pub fn key(&self) -> ResourceKey {
        ResourceKey {
            connection_id: self.connection_id.clone(),
            kind: self.kind.clone(),
            external_id: self.external_id.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceVersion {
    pub version_id: ResourceVersionId,
    pub resource_id: ResourceId,
    pub observed_at: Timestamp,
    pub sync_run_id: SyncRunId,
    pub normalized_snapshot: Value,
    pub fingerprint: Fingerprint,
    pub schema_version: SchemaVersion,
    pub change_summary: Vec<FieldChange>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Relation {
    pub relation_id: RelationId,
    pub source_resource_id: ResourceId,
    pub target_resource_id: ResourceId,
    pub kind: RelationKind,
    pub evidence_key: EvidenceKey,
    pub evidence: RelationEvidence,
    pub first_seen_at: Timestamp,
    pub last_seen_at: Timestamp,
    pub lifecycle: Lifecycle,
}

impl Relation {
    pub fn key(&self) -> RelationKey {
        RelationKey {
            source_resource_id: self.source_resource_id.clone(),
            target_resource_id: self.target_resource_id.clone(),
            kind: self.kind.clone(),
            evidence_type: self.evidence.evidence_type(),
            evidence_key: self.evidence_key.clone(),
        }
    }

    pub fn last_sync_run_id(&self) -> Option<&SyncRunId> {
        self.evidence.sync_run_id()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationVersion {
    pub relation_version_id: RelationVersionId,
    pub relation_id: RelationId,
    pub observed_at: Timestamp,
    pub normalized_snapshot: Value,
    pub fingerprint: Fingerprint,
    pub schema_version: SchemaVersion,
    pub origin: OriginRef,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    pub binding_id: BindingId,
    pub source_resource_id: ResourceId,
    pub target_resource_id: ResourceId,
    pub kind: RelationKind,
    pub status: BindingStatus,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InferenceRun {
    pub inference_run_id: InferenceRunId,
    pub rule_version: RuleVersion,
    pub started_at: Timestamp,
    pub finished_at: Option<Timestamp>,
    pub status: InferenceRunStatus,
    pub input_resource_version_ids: Vec<ResourceVersionId>,
    pub input_relation_version_ids: Vec<RelationVersionId>,
    pub output_relation_ids: Vec<RelationId>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncRunCounts {
    pub read: u64,
    pub created: u64,
    pub updated: u64,
    pub unchanged: u64,
    pub warnings: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncRunWarning {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncRun {
    pub sync_run_id: SyncRunId,
    pub connection_id: ConnectionId,
    pub mode: SyncMode,
    pub trigger: SyncTrigger,
    pub started_at: Timestamp,
    pub finished_at: Option<Timestamp>,
    pub status: SyncRunStatus,
    pub coverage: SyncCoverage,
    pub cursor_before: Option<SyncCursor>,
    pub cursor_after: Option<SyncCursor>,
    pub counts: SyncRunCounts,
    pub errors: Vec<DomainError>,
    pub warnings: Vec<SyncRunWarning>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChangeSubject {
    Resource { resource_id: ResourceId },
    Relation { relation_id: RelationId },
    Binding { binding_id: BindingId },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldChange {
    pub path: FieldPath,
    pub before: Option<Value>,
    pub after: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    pub change_id: ChangeId,
    pub subject: ChangeSubject,
    pub observed_at: Timestamp,
    pub fields: Vec<FieldChange>,
    pub origin: OriginRef,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    InspectSummary,
    InspectTopology,
    InspectVersions,
    InspectProviderDetails,
}
