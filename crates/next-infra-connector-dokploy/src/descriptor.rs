use next_infra_connector_api::{
    AuthDescriptor, AuthKind, ConnectorDescriptor, RateLimitGuidance, RelationCapability,
    ResourceCapability,
};
use next_infra_core::{
    ConnectorCoverage, ConnectorCoverageLevel, ConnectorType, RelationKind, ResourceKind,
    SchemaVersion, SyncMode,
};

pub fn dokploy_descriptor() -> ConnectorDescriptor {
    let project = kind("dokploy.project");
    let application = kind("dokploy.application");
    let deployment = kind("dokploy.deployment");
    let server = kind("dokploy.server");
    let domain = kind("dokploy.domain");
    let database = kind("dokploy.database");
    ConnectorDescriptor {
        connector_type: ConnectorType::new("dokploy").expect("static connector type"),
        connector_version: "1.0.0".into(),
        config_schema_version: SchemaVersion::new(1).expect("static schema version"),
        auth: AuthDescriptor {
            kind: AuthKind::Token,
            minimum_permissions: vec!["Read-only control-plane access to projects and deployments".into()],
        },
        sync_modes: vec![SyncMode::Full],
        resources: vec![
            resource(project.clone(), "dokploy.projects", ConnectorCoverageLevel::Supported, None),
            resource(application.clone(), "dokploy.applications", ConnectorCoverageLevel::Supported, None),
            resource(deployment.clone(), "dokploy.deployments", ConnectorCoverageLevel::Partial, Some("deployment history is bounded to the current control-plane response")),
            resource(server.clone(), "dokploy.servers", ConnectorCoverageLevel::Supported, None),
            resource(domain.clone(), "dokploy.domains", ConnectorCoverageLevel::Supported, None),
            resource(database, "dokploy.database", ConnectorCoverageLevel::Unsupported, Some("excluded by DEC-G8-01 pending a credential-safe field allowlist")),
        ],
        relations: vec![
            relation(&project, &application, "dokploy.contains", "dokploy.project_application"),
            relation(&application, &deployment, "dokploy.deploys", "dokploy.application_deployment"),
            relation(&application, &server, "dokploy.runs_on", "dokploy.application_server"),
            relation(&application, &domain, "dokploy.exposes", "dokploy.application_domain"),
        ],
        sensitive_field_policy: vec![
            "tokens are accepted only as ephemeral SecretValue input".into(),
            "passwords, connection strings, environment variables, logs and raw response bodies are never persisted".into(),
            "dokploy.database is unsupported by DEC-G8-01".into(),
        ],
        rate_limit: RateLimitGuidance { default_max_concurrency: 2, requests_per_minute: None, respects_retry_after: true },
        recommended_sync_interval_secs: 900,
        known_gaps: vec![
            "Database resources are unsupported pending DEC-G8-01 expansion".into(),
            "write APIs, logs, environment variables and application source are unsupported".into(),
        ],
    }
}

fn kind(value: &str) -> ResourceKind {
    ResourceKind::new(value).expect("static resource kind")
}

fn resource(
    kind: ResourceKind,
    module: &str,
    level: ConnectorCoverageLevel,
    reason: Option<&str>,
) -> ResourceCapability {
    ResourceCapability {
        kind,
        attribute_schema_version: SchemaVersion::new(1).expect("static schema version"),
        coverage: ConnectorCoverage {
            module: module.into(),
            level,
            reason: reason.map(str::to_owned),
        },
    }
}

fn relation(
    source_kind: &ResourceKind,
    target_kind: &ResourceKind,
    relation_kind: &str,
    module: &str,
) -> RelationCapability {
    RelationCapability {
        kind: RelationKind::new(relation_kind).expect("static relation kind"),
        source_kind: source_kind.clone(),
        target_kind: target_kind.clone(),
        coverage: ConnectorCoverage {
            module: module.into(),
            level: ConnectorCoverageLevel::Supported,
            reason: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use next_infra_connector_contract_tests::check_descriptor;

    #[test]
    fn descriptor_is_valid_and_database_is_explicitly_unsupported() {
        let descriptor = dokploy_descriptor();
        assert!(descriptor.validate().is_ok());
        assert!(check_descriptor(&descriptor).is_empty());
        let database = descriptor
            .resources
            .iter()
            .find(|resource| resource.kind.as_str() == "dokploy.database")
            .unwrap();
        assert_eq!(database.coverage.level, ConnectorCoverageLevel::Unsupported);
        assert!(
            database
                .coverage
                .reason
                .as_deref()
                .unwrap()
                .contains("DEC-G8-01")
        );
    }

    #[test]
    fn descriptor_does_not_claim_secret_reads() {
        let serialized = serde_json::to_string(&dokploy_descriptor())
            .unwrap()
            .to_ascii_lowercase();
        for forbidden in ["connection string", "environment variables", "logs"] {
            assert!(serialized.contains(forbidden));
        }
        assert!(serialized.contains("never persisted"));
    }
}
