use next_infra_core::{
    ConnectorType, ErrorCode, EvidenceKey, ExternalId, FieldPath, LabelKey, RelationKind,
    ResourceHealth, ResourceKind, SchemaVersion, Scope, SyncCoverage, SyncCursor, SyncMode,
    SyncRunId, Timestamp,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionInput {
    pub connection_id: next_infra_core::ConnectionId,
    pub connector_type: ConnectorType,
    pub config: Value,
    pub config_schema_version: SchemaVersion,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationRequest {
    pub connection: ConnectionInput,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationStatus {
    Valid,
    Invalid,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationReport {
    pub status: ValidationStatus,
    pub warnings: Vec<ValidationIssue>,
    pub errors: Vec<ValidationIssue>,
}

impl ValidationReport {
    pub fn validate(&self) -> Result<(), next_infra_core::DomainError> {
        let coherent = match self.status {
            ValidationStatus::Valid => self.errors.is_empty(),
            ValidationStatus::Invalid => !self.errors.is_empty(),
        };
        if coherent {
            Ok(())
        } else {
            Err(next_infra_core::DomainError::invalid_value(
                "validation status and errors disagree",
            ))
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceLocator {
    pub kind: ResourceKind,
    pub external_id: ExternalId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncRequest {
    pub sync_run_id: SyncRunId,
    pub connection: ConnectionInput,
    pub mode: SyncMode,
    pub scope: Scope,
    pub cursor: Option<SyncCursor>,
    pub targeted_resources: Vec<ResourceLocator>,
}

impl SyncRequest {
    pub fn accepts_coverage(&self, coverage: &SyncCoverage) -> bool {
        matches!(
            (self.mode, coverage),
            (SyncMode::Full, SyncCoverage::AuthoritativeFull { .. })
                | (SyncMode::Full, SyncCoverage::Partial { .. })
                | (SyncMode::Incremental, SyncCoverage::Incremental { .. })
                | (SyncMode::Incremental, SyncCoverage::Partial { .. })
                | (SyncMode::Targeted, SyncCoverage::Targeted { .. })
                | (SyncMode::Targeted, SyncCoverage::Partial { .. })
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceObservation {
    pub kind: ResourceKind,
    pub external_id: ExternalId,
    pub name: String,
    pub display_name: String,
    pub scope: Scope,
    pub labels: BTreeMap<LabelKey, String>,
    pub health: ResourceHealth,
    pub attributes: Value,
    pub attribute_schema_version: SchemaVersion,
    pub observed_at: Timestamp,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationObservation {
    pub source: ResourceLocator,
    pub target: ResourceLocator,
    pub kind: RelationKind,
    pub evidence_key: EvidenceKey,
    pub field_path: FieldPath,
    pub observed_at: Timestamp,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactionReport {
    pub removed_fields: u64,
    pub unknown_fields_dropped: u64,
    pub secret_sentinels_detected: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderRequestSummary {
    pub request_count: u64,
    pub elapsed_ms: u64,
    pub status_class_counts: BTreeMap<String, u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservationWarning {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservationBatch {
    pub resources: Vec<ResourceObservation>,
    pub relations: Vec<RelationObservation>,
    pub coverage: SyncCoverage,
    pub next_cursor: Option<SyncCursor>,
    pub warnings: Vec<ObservationWarning>,
    pub redaction_report: RedactionReport,
    pub provider_request_summary: ProviderRequestSummary,
}

impl ObservationBatch {
    pub fn validate_for(&self, request: &SyncRequest) -> Result<(), next_infra_core::DomainError> {
        if !request.accepts_coverage(&self.coverage) {
            return Err(next_infra_core::DomainError::invalid_value(
                "observation coverage is incompatible with sync mode",
            ));
        }
        if self.redaction_report.secret_sentinels_detected != 0 {
            return Err(next_infra_core::DomainError::invalid_value(
                "observation batch contains a secret sentinel",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SyncOutcome {
    Complete {
        batch: ObservationBatch,
    },
    Partial {
        batch: ObservationBatch,
        failure: crate::ConnectorFailure,
    },
}

impl SyncOutcome {
    pub fn batch(&self) -> &ObservationBatch {
        match self {
            Self::Complete { batch } | Self::Partial { batch, .. } => batch,
        }
    }

    pub fn validate_for(&self, request: &SyncRequest) -> Result<(), next_infra_core::DomainError> {
        self.batch().validate_for(request)?;
        let partial_coverage = matches!(self.batch().coverage, SyncCoverage::Partial { .. });
        match self {
            Self::Complete { .. } if partial_coverage => {
                Err(next_infra_core::DomainError::invalid_value(
                    "complete outcome cannot contain partial coverage",
                ))
            }
            Self::Partial { .. } if !partial_coverage => {
                Err(next_infra_core::DomainError::invalid_value(
                    "partial outcome must contain partial coverage",
                ))
            }
            Self::Complete { .. } | Self::Partial { .. } => Ok(()),
        }
    }
}
