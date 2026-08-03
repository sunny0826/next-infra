//! Deterministic Connector Coverage catalog projection.

use next_infra_connector_api::{
    AuthDescriptor, ConnectorDescriptor, RateLimitGuidance, RelationCapability, ResourceCapability,
};
use next_infra_core::{
    ConnectorCoverageLevel, ConnectorType, DomainError, RelationKind, ResourceKind, SyncMode,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectorCoverageSnapshot {
    pub connector_type: ConnectorType,
    pub connector_version: String,
    pub auth: AuthDescriptor,
    pub sync_modes: Vec<SyncMode>,
    pub rate_limit: RateLimitGuidance,
    pub known_gaps: Vec<String>,
    pub modules: Vec<ModuleCoverageSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleCoverageSnapshot {
    pub module: String,
    pub level: ConnectorCoverageLevel,
    pub reason: Option<String>,
    pub subject: CoverageSubject,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CoverageSubject {
    Resource {
        kind: ResourceKind,
        attribute_schema_version: u32,
    },
    Relation {
        kind: RelationKind,
        source_kind: ResourceKind,
        target_kind: ResourceKind,
    },
}

impl ConnectorCoverageSnapshot {
    pub fn from_descriptor(descriptor: &ConnectorDescriptor) -> Result<Self, DomainError> {
        descriptor.validate()?;
        let mut modules = descriptor
            .resources
            .iter()
            .map(resource_module)
            .chain(descriptor.relations.iter().map(relation_module))
            .collect::<Vec<_>>();
        modules.sort_by_key(module_key);
        Ok(Self {
            connector_type: descriptor.connector_type.clone(),
            connector_version: descriptor.connector_version.clone(),
            auth: descriptor.auth.clone(),
            sync_modes: descriptor.sync_modes.clone(),
            rate_limit: descriptor.rate_limit.clone(),
            known_gaps: descriptor.known_gaps.clone(),
            modules,
        })
    }
}

fn resource_module(capability: &ResourceCapability) -> ModuleCoverageSnapshot {
    ModuleCoverageSnapshot {
        module: capability.coverage.module.clone(),
        level: capability.coverage.level,
        reason: capability.coverage.reason.clone(),
        subject: CoverageSubject::Resource {
            kind: capability.kind.clone(),
            attribute_schema_version: capability.attribute_schema_version.get(),
        },
    }
}

fn relation_module(capability: &RelationCapability) -> ModuleCoverageSnapshot {
    ModuleCoverageSnapshot {
        module: capability.coverage.module.clone(),
        level: capability.coverage.level,
        reason: capability.coverage.reason.clone(),
        subject: CoverageSubject::Relation {
            kind: capability.kind.clone(),
            source_kind: capability.source_kind.clone(),
            target_kind: capability.target_kind.clone(),
        },
    }
}

fn module_key(module: &ModuleCoverageSnapshot) -> String {
    let subject = serde_json::to_string(&module.subject).expect("coverage subject is serializable");
    format!("{}:{subject}", module.module)
}

#[cfg(test)]
mod tests {
    use super::*;
    use next_infra_connector_api::*;
    use next_infra_core::*;

    #[test]
    fn catalog_keeps_coverage_separate_from_runtime_state() {
        let descriptor = ConnectorDescriptor {
            connector_type: ConnectorType::new("fixture").unwrap(),
            connector_version: "1.0.0".into(),
            config_schema_version: SchemaVersion::new(1).unwrap(),
            auth: AuthDescriptor {
                kind: AuthKind::None,
                minimum_permissions: Vec::new(),
            },
            sync_modes: vec![SyncMode::Full],
            resources: vec![ResourceCapability {
                kind: ResourceKind::new("fixture.resource").unwrap(),
                attribute_schema_version: SchemaVersion::new(1).unwrap(),
                coverage: ConnectorCoverage {
                    module: "fixture.resources".into(),
                    level: ConnectorCoverageLevel::Partial,
                    reason: Some("fixture gap".into()),
                },
            }],
            relations: Vec::new(),
            sensitive_field_policy: Vec::new(),
            rate_limit: RateLimitGuidance {
                default_max_concurrency: 1,
                requests_per_minute: None,
                respects_retry_after: true,
            },
            recommended_sync_interval_secs: 60,
            known_gaps: vec!["fixture gap".into()],
        };

        let snapshot = ConnectorCoverageSnapshot::from_descriptor(&descriptor).unwrap();
        assert_eq!(snapshot.modules[0].level, ConnectorCoverageLevel::Partial);
        let serialized = serde_json::to_string(&snapshot).unwrap();
        assert!(!serialized.contains("health"));
        assert!(!serialized.contains("sync_run"));
    }
}
