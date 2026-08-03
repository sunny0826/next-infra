use crate::{ResourceId, Scope, SyncCursor};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SyncCoverage {
    AuthoritativeFull {
        scope: Scope,
    },
    Incremental {
        cursor: SyncCursor,
    },
    Partial {
        scope: Option<Scope>,
        reason: CoverageGapReason,
    },
    Targeted {
        resource_ids: Vec<ResourceId>,
    },
}

impl SyncCoverage {
    pub const fn contributes_missing_evidence(&self) -> bool {
        matches!(self, Self::AuthoritativeFull { .. })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "detail", rename_all = "snake_case")]
pub enum CoverageGapReason {
    PermissionDenied,
    PaginationIncomplete,
    RateLimited,
    ProviderUnavailable,
    SchemaIncompatible,
    Other(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorCoverageLevel {
    Supported,
    Partial,
    Unsupported,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectorCoverage {
    pub module: String,
    pub level: ConnectorCoverageLevel,
    pub reason: Option<String>,
}
