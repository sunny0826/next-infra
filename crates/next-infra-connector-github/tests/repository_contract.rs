use next_infra_connector_api::{
    ConnectionInput, ObservationBatch, ProviderRequestSummary, RedactionReport, SyncRequest,
};
use next_infra_connector_contract_tests::check_batch;
use next_infra_connector_github::{
    deployment::{DeploymentDto, map_deployments},
    environment::{EnvironmentDto, map_environments},
    repository::{RepositoryDto, map_repositories},
};
use next_infra_core::{
    ConnectorType, CoverageGapReason, ResourceKind, SchemaVersion, Scope, SyncCoverage, SyncMode,
    SyncRunId, Timestamp,
};
use next_infra_normalizer::{AttributeSchema, Normalizer, RelationSchema};
use serde_json::json;
use std::collections::BTreeSet;

#[test]
fn repository_resources_drop_unknown_fields_and_pass_normalizer() {
    let repository_json = json!({
        "id": 10,
        "name": "fixture-repo",
        "owner": {"login": "fixture-owner", "html_url": "https://example.test/owner"},
        "visibility": "private",
        "default_branch": null,
        "archived": false,
        "disabled": false,
        "created_at": "2026-08-05T00:00:00Z",
        "updated_at": "2026-08-05T00:01:00Z",
        "permissions": {"admin": true},
        "clone_url": "https://secret-sentinel.example.test/repo",
        "ssh_url": "secret-sentinel",
        "authorization": "Bearer secret-sentinel"
    });
    let repository: RepositoryDto = serde_json::from_value(repository_json).unwrap();
    let scope = Scope::new("github-account-scope").unwrap();
    let observed_at = Timestamp::from_unix_millis(1_000).unwrap();
    let repositories = map_repositories(&scope, observed_at, [repository], false, None).unwrap();
    let context = repositories.routes[0].clone();

    let environment_json = json!({
        "id": 20,
        "name": "fixture-environment",
        "deployment_branch_policy": {"protected_branches": true, "custom_branch_policies": false},
        "protection_rules": [{"reviewers": [{"type": "User", "reviewer": {"login": "secret-sentinel"}}]}],
        "html_url": "https://secret-sentinel.example.test/environment"
    });
    let environment: EnvironmentDto = serde_json::from_value(environment_json).unwrap();
    let environments = map_environments(&context, [environment], false, None).unwrap();

    let deployment_json = json!({
        "id": 30,
        "environment": "fixture-environment",
        "task": "deploy",
        "transient_environment": false,
        "production_environment": true,
        "created_at": "2026-08-05T00:00:00Z",
        "updated_at": "2026-08-05T00:01:00Z",
        "payload": {"token": "secret-sentinel"},
        "creator": {"login": "secret-sentinel"},
        "sha": "secret-sentinel",
        "statuses_url": "https://secret-sentinel.example.test/statuses"
    });
    let deployment: DeploymentDto = serde_json::from_value(deployment_json).unwrap();
    let deployments = map_deployments(&context, [deployment], false, None).unwrap();

    let output = repositories.merge(environments).merge(deployments);
    let serialized = serde_json::to_string(&output.resources)
        .unwrap()
        .to_ascii_lowercase();
    for forbidden in [
        "secret-sentinel",
        "permissions",
        "clone_url",
        "ssh_url",
        "protection_rules",
        "reviewers",
        "payload",
        "creator",
        "statuses_url",
    ] {
        assert!(!serialized.contains(forbidden), "found {forbidden}");
    }

    let batch = ObservationBatch {
        resources: output.resources,
        relations: output.relations,
        coverage: SyncCoverage::Partial {
            scope: Some(scope.clone()),
            reason: CoverageGapReason::Other("deployment status unsupported".into()),
        },
        next_cursor: None,
        warnings: output.warnings,
        redaction_report: RedactionReport::default(),
        provider_request_summary: ProviderRequestSummary::default(),
    };
    assert!(check_batch(&batch).is_empty());

    let normalizer = Normalizer::new(
        [
            schema(
                "github.repository",
                &[
                    "repository_id",
                    "visibility",
                    "default_branch",
                    "archived",
                    "disabled",
                    "created_at",
                    "updated_at",
                ],
            ),
            schema(
                "github.environment",
                &[
                    "environment_id",
                    "repository_id",
                    "protected_branches",
                    "custom_branch_policies",
                ],
            ),
            schema(
                "github.deployment",
                &[
                    "deployment_id",
                    "repository_id",
                    "environment",
                    "task",
                    "transient_environment",
                    "production_environment",
                    "created_at",
                    "updated_at",
                ],
            ),
        ],
        [
            relation("github.contains", "github.repository", "github.environment"),
            relation("github.contains", "github.repository", "github.deployment"),
        ],
    )
    .unwrap();
    let request = SyncRequest {
        sync_run_id: SyncRunId::new("github-repository-fixture-run").unwrap(),
        connection: ConnectionInput {
            connection_id: next_infra_core::ConnectionId::new("github-fixture-connection").unwrap(),
            connector_type: ConnectorType::new("github").unwrap(),
            config: json!({"selected_repository_ids": ["10"]}),
            config_schema_version: SchemaVersion::new(1).unwrap(),
        },
        mode: SyncMode::Full,
        scope,
        cursor: None,
        targeted_resources: Vec::new(),
    };
    let normalized = normalizer.normalize(&request, batch).unwrap();
    assert_eq!(normalized.resources.len(), 3);
    assert_eq!(normalized.relations.len(), 2);
    assert_eq!(normalized.redaction_report.secret_sentinels_detected, 0);
}

#[test]
fn environment_and_deployment_budgets_are_enforced() {
    let repository: RepositoryDto = serde_json::from_value(json!({
        "id": 10,
        "name": "fixture-repo",
        "owner": {"login": "fixture-owner"},
        "visibility": "private",
        "default_branch": null,
        "archived": false,
        "disabled": false,
        "created_at": "2026-08-05T00:00:00Z",
        "updated_at": "2026-08-05T00:01:00Z"
    }))
    .unwrap();
    let repositories = map_repositories(
        &Scope::new("github-account-scope").unwrap(),
        Timestamp::from_unix_millis(1_000).unwrap(),
        [repository],
        false,
        None,
    )
    .unwrap();
    let context = &repositories.routes[0];
    let environments = (0
        ..=next_infra_connector_github::environment::MAX_ENVIRONMENTS_PER_REPOSITORY)
        .map(|index| EnvironmentDto {
            id: u64::try_from(index).unwrap(),
            name: format!("fixture-environment-{index}"),
            deployment_branch_policy: None,
        })
        .collect::<Vec<_>>();
    let output = map_environments(context, environments, false, None).unwrap();
    assert_eq!(
        output.resources.len(),
        next_infra_connector_github::environment::MAX_ENVIRONMENTS_PER_REPOSITORY
    );
    assert!(output.modules[0].bounded);

    let deployments = (0..=next_infra_connector_github::deployment::MAX_DEPLOYMENTS_PER_REPOSITORY)
        .map(|index| DeploymentDto {
            id: u64::try_from(index).unwrap(),
            environment: None,
            task: "deploy".into(),
            transient_environment: false,
            production_environment: false,
            created_at: "2026-08-05T00:00:00Z".into(),
            updated_at: "2026-08-05T00:01:00Z".into(),
        })
        .collect::<Vec<_>>();
    let output = map_deployments(context, deployments, false, None).unwrap();
    assert_eq!(
        output.resources.len(),
        next_infra_connector_github::deployment::MAX_DEPLOYMENTS_PER_REPOSITORY
    );
    assert!(output.modules[0].bounded);
}

fn schema(kind: &str, attributes: &[&str]) -> AttributeSchema {
    AttributeSchema {
        kind: ResourceKind::new(kind).unwrap(),
        schema_version: SchemaVersion::new(1).unwrap(),
        allowed_attributes: attributes
            .iter()
            .map(|value| (*value).into())
            .collect::<BTreeSet<_>>(),
    }
}

fn relation(kind: &str, source: &str, target: &str) -> RelationSchema {
    RelationSchema {
        kind: next_infra_core::RelationKind::new(kind).unwrap(),
        source_kind: ResourceKind::new(source).unwrap(),
        target_kind: ResourceKind::new(target).unwrap(),
    }
}
