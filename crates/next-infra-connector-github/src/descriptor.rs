use next_infra_connector_api::{
    AuthDescriptor, AuthKind, ConnectorDescriptor, RateLimitGuidance, RelationCapability,
    ResourceCapability,
};
use next_infra_core::{
    ConnectorCoverage, ConnectorCoverageLevel, ConnectorType, RelationKind, ResourceKind,
    SchemaVersion, SyncMode,
};

pub fn github_descriptor() -> ConnectorDescriptor {
    let repository = kind("github.repository");
    let environment = kind("github.environment");
    let deployment = kind("github.deployment");
    let workflow = kind("github.workflow");
    let run = kind("github.workflow_run");
    let job = kind("github.workflow_job");

    ConnectorDescriptor {
        connector_type: ConnectorType::new("github").expect("static connector type"),
        connector_version: "1.0.0".into(),
        config_schema_version: SchemaVersion::new(1).expect("static schema version"),
        auth: AuthDescriptor {
            kind: AuthKind::Token,
            minimum_permissions: vec![
                "Metadata: read".into(),
                "Actions: read".into(),
                "Deployments: read".into(),
            ],
        },
        sync_modes: vec![SyncMode::Full, SyncMode::Targeted],
        resources: vec![
            resource(
                repository.clone(),
                "github.repositories",
                "CON-G5-02 mapper pending",
            ),
            resource(
                environment.clone(),
                "github.environments",
                "CON-G5-02 mapper pending",
            ),
            resource(
                deployment.clone(),
                "github.deployments",
                "CON-G5-02 mapper pending",
            ),
            resource(
                workflow.clone(),
                "github.actions.workflows",
                "CON-G5-03 mapper pending",
            ),
            resource(
                run.clone(),
                "github.actions.runs",
                "CON-G5-03 mapper pending",
            ),
            resource(
                job.clone(),
                "github.actions.jobs",
                "CON-G5-03 mapper pending",
            ),
        ],
        relations: vec![
            relation(
                &repository,
                &environment,
                "github.contains",
                "github.repository_environment",
                "CON-G5-02 mapper pending",
            ),
            relation(
                &repository,
                &deployment,
                "github.contains",
                "github.repository_deployment",
                "CON-G5-02 mapper pending",
            ),
            relation(
                &repository,
                &workflow,
                "github.contains",
                "github.repository_workflow",
                "CON-G5-03 mapper pending",
            ),
            relation(
                &workflow,
                &run,
                "github.executes",
                "github.workflow_run",
                "CON-G5-03 mapper pending",
            ),
            relation(
                &run,
                &job,
                "github.contains",
                "github.run_job",
                "CON-G5-03 mapper pending",
            ),
        ],
        sensitive_field_policy: vec![
            "tokens are accepted only as ephemeral SecretValue input".into(),
            "logs, artifacts, secrets and variables are never requested".into(),
            "raw response bodies are not persisted or logged".into(),
        ],
        rate_limit: RateLimitGuidance {
            default_max_concurrency: 2,
            requests_per_minute: None,
            respects_retry_after: true,
        },
        recommended_sync_interval_secs: 900,
        known_gaps: vec![
            "repository/environment/deployment mapper pending CON-G5-02".into(),
            "workflow/run/job mapper pending CON-G5-03".into(),
            "logs, artifacts, secrets, variables and write APIs are unsupported".into(),
            "GitHub Enterprise Server base URLs are unsupported".into(),
        ],
    }
}

fn kind(value: &str) -> ResourceKind {
    ResourceKind::new(value).expect("static resource kind")
}

fn resource(kind: ResourceKind, module: &str, reason: &str) -> ResourceCapability {
    ResourceCapability {
        kind,
        attribute_schema_version: SchemaVersion::new(1).expect("static schema version"),
        coverage: ConnectorCoverage {
            module: module.into(),
            level: ConnectorCoverageLevel::Partial,
            reason: Some(reason.into()),
        },
    }
}

fn relation(
    source_kind: &ResourceKind,
    target_kind: &ResourceKind,
    relation_kind: &str,
    module: &str,
    reason: &str,
) -> RelationCapability {
    RelationCapability {
        kind: RelationKind::new(relation_kind).expect("static relation kind"),
        source_kind: source_kind.clone(),
        target_kind: target_kind.clone(),
        coverage: ConnectorCoverage {
            module: module.into(),
            level: ConnectorCoverageLevel::Partial,
            reason: Some(reason.into()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use next_infra_connector_contract_tests::check_descriptor;

    #[test]
    fn descriptor_is_valid_and_does_not_claim_mapper_completion() {
        let descriptor = github_descriptor();
        assert!(descriptor.validate().is_ok());
        assert!(check_descriptor(&descriptor).is_empty());
        assert!(
            descriptor
                .resources
                .iter()
                .all(|capability| { capability.coverage.level == ConnectorCoverageLevel::Partial })
        );
        let serialized = serde_json::to_string(&descriptor)
            .unwrap()
            .to_ascii_lowercase();
        for forbidden in ["contents: read", "administration", "write"] {
            if forbidden == "write" {
                assert!(serialized.contains("write apis are unsupported"));
            } else {
                assert!(!serialized.contains(forbidden));
            }
        }
    }
}
