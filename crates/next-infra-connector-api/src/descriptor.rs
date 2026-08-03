use next_infra_core::{
    ConnectorCoverage, ConnectorType, DomainError, RelationKind, ResourceKind, SchemaVersion,
    SyncMode,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthKind {
    None,
    Token,
    ApiKey,
    Oauth,
    SshAgent,
    SshKey,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthDescriptor {
    pub kind: AuthKind,
    pub minimum_permissions: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceCapability {
    pub kind: ResourceKind,
    pub attribute_schema_version: SchemaVersion,
    pub coverage: ConnectorCoverage,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationCapability {
    pub kind: RelationKind,
    pub source_kind: ResourceKind,
    pub target_kind: ResourceKind,
    pub coverage: ConnectorCoverage,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RateLimitGuidance {
    pub default_max_concurrency: u16,
    pub requests_per_minute: Option<u32>,
    pub respects_retry_after: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectorDescriptor {
    pub connector_type: ConnectorType,
    pub connector_version: String,
    pub config_schema_version: SchemaVersion,
    pub auth: AuthDescriptor,
    pub sync_modes: Vec<SyncMode>,
    pub resources: Vec<ResourceCapability>,
    pub relations: Vec<RelationCapability>,
    pub sensitive_field_policy: Vec<String>,
    pub rate_limit: RateLimitGuidance,
    pub recommended_sync_interval_secs: u64,
    pub known_gaps: Vec<String>,
}

impl ConnectorDescriptor {
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.connector_version.trim().is_empty() {
            return Err(DomainError::invalid_value(
                "connector_version cannot be empty",
            ));
        }
        if self.sync_modes.is_empty() {
            return Err(DomainError::invalid_value(
                "connector must support at least one sync mode",
            ));
        }
        let has_duplicate_mode = self
            .sync_modes
            .iter()
            .enumerate()
            .any(|(index, mode)| self.sync_modes[index + 1..].contains(mode));
        if has_duplicate_mode {
            return Err(DomainError::invalid_value(
                "connector sync_modes contain duplicates",
            ));
        }
        if self.rate_limit.default_max_concurrency == 0 {
            return Err(DomainError::invalid_value(
                "default_max_concurrency must be positive",
            ));
        }
        if self.recommended_sync_interval_secs == 0 {
            return Err(DomainError::invalid_value(
                "recommended_sync_interval_secs must be positive",
            ));
        }

        let unique_resources = self
            .resources
            .iter()
            .map(|capability| &capability.kind)
            .collect::<BTreeSet<_>>();
        if unique_resources.len() != self.resources.len() {
            return Err(DomainError::invalid_value(
                "resource capabilities contain duplicate kinds",
            ));
        }

        let unique_relations = self
            .relations
            .iter()
            .map(|capability| {
                (
                    &capability.source_kind,
                    &capability.target_kind,
                    &capability.kind,
                )
            })
            .collect::<BTreeSet<_>>();
        if unique_relations.len() != self.relations.len() {
            return Err(DomainError::invalid_value(
                "relation capabilities contain duplicate endpoints and kind",
            ));
        }

        Ok(())
    }
}
