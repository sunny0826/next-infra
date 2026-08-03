use crate::{
    BindingId, Confidence, ConnectionId, FieldPath, RelationVersionId, ResourceVersionId,
    RuleVersion, SyncRunId,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceType {
    Provider,
    Configured,
    Inferred,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RelationEvidence {
    Provider {
        connection_id: ConnectionId,
        sync_run_id: SyncRunId,
        field_path: FieldPath,
    },
    Configured {
        binding_id: BindingId,
    },
    Inferred {
        rule_version: RuleVersion,
        input_resource_version_ids: Vec<ResourceVersionId>,
        #[serde(default)]
        input_relation_version_ids: Vec<RelationVersionId>,
        confidence: Confidence,
    },
}

impl RelationEvidence {
    pub const fn evidence_type(&self) -> EvidenceType {
        match self {
            Self::Provider { .. } => EvidenceType::Provider,
            Self::Configured { .. } => EvidenceType::Configured,
            Self::Inferred { .. } => EvidenceType::Inferred,
        }
    }

    pub const fn sync_run_id(&self) -> Option<&SyncRunId> {
        match self {
            Self::Provider { sync_run_id, .. } => Some(sync_run_id),
            Self::Configured { .. } | Self::Inferred { .. } => None,
        }
    }

    pub const fn confidence(&self) -> Option<Confidence> {
        match self {
            Self::Inferred { confidence, .. } => Some(*confidence),
            Self::Provider { .. } | Self::Configured { .. } => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OriginRef {
    SyncRun {
        sync_run_id: SyncRunId,
    },
    Binding {
        binding_id: BindingId,
    },
    Inference {
        rule_version: RuleVersion,
        input_resource_version_ids: Vec<ResourceVersionId>,
    },
}
