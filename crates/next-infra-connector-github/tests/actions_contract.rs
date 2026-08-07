use next_infra_connector_api::{
    ConnectionInput, ObservationBatch, ProviderRequestSummary, RedactionReport, SyncRequest,
};
use next_infra_connector_contract_tests::check_batch;
use next_infra_connector_github::actions::{
    GitHubRepositoryContext, JobDto, WorkflowDto, WorkflowRunDto, map_jobs, map_runs, map_workflows,
};
use next_infra_core::{
    ConnectorType, CoverageGapReason, ExternalId, ResourceKind, SchemaVersion, Scope, SyncCoverage,
    SyncMode, SyncRunId, Timestamp,
};
use next_infra_normalizer::{AttributeSchema, Normalizer, RelationSchema};
use serde_json::json;
use std::collections::BTreeSet;

#[test]
fn actions_output_passes_common_conformance_and_normalizer() {
    let context = GitHubRepositoryContext {
        repository_external_id: ExternalId::new("github-repository:10").unwrap(),
        scope: Scope::new("github-repository-scope:10").unwrap(),
        observed_at: Timestamp::from_unix_millis(1_000).unwrap(),
    };
    let workflows = map_workflows(
        &context,
        [WorkflowDto {
            id: 20,
            name: "Fixture workflow".into(),
            path: ".github/workflows/fixture.yml".into(),
            state: "active".into(),
            created_at: "2026-08-05T00:00:00Z".into(),
            updated_at: "2026-08-05T00:01:00Z".into(),
        }],
        false,
        None,
    )
    .unwrap();
    let runs = map_runs(
        &context,
        [WorkflowRunDto {
            id: 30,
            workflow_id: 20,
            name: Some("Fixture workflow".into()),
            display_title: "Fixture run".into(),
            run_number: 1,
            run_attempt: 1,
            event: "push".into(),
            status: "completed".into(),
            conclusion: Some("success".into()),
            head_branch: Some("fixture-branch".into()),
            created_at: "2026-08-05T00:00:00Z".into(),
            updated_at: "2026-08-05T00:01:00Z".into(),
            run_started_at: Some("2026-08-05T00:00:10Z".into()),
        }],
        true,
        None,
    )
    .unwrap();
    let jobs = map_jobs(
        &context,
        [JobDto {
            id: 40,
            run_id: 30,
            name: "Fixture job".into(),
            status: "completed".into(),
            conclusion: Some("success".into()),
            started_at: Some("2026-08-05T00:00:20Z".into()),
            completed_at: Some("2026-08-05T00:00:50Z".into()),
        }],
        false,
        None,
    )
    .unwrap();
    let output = workflows.merge(runs).merge(jobs);
    let batch = ObservationBatch {
        resources: output.resources,
        relations: output.relations,
        coverage: SyncCoverage::Partial {
            scope: Some(context.scope.clone()),
            reason: CoverageGapReason::Other("bounded actions history".into()),
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
                "github.workflow",
                &["workflow_id", "path", "state", "created_at", "updated_at"],
            ),
            schema(
                "github.workflow_run",
                &[
                    "run_id",
                    "workflow_id",
                    "run_number",
                    "run_attempt",
                    "event",
                    "status",
                    "conclusion",
                    "head_branch",
                    "created_at",
                    "updated_at",
                    "run_started_at",
                ],
            ),
            schema(
                "github.workflow_job",
                &[
                    "job_id",
                    "run_id",
                    "status",
                    "conclusion",
                    "started_at",
                    "completed_at",
                ],
            ),
        ],
        [
            relation("github.contains", "github.repository", "github.workflow"),
            relation("github.executes", "github.workflow", "github.workflow_run"),
            relation(
                "github.contains",
                "github.workflow_run",
                "github.workflow_job",
            ),
        ],
    )
    .unwrap();
    let request = SyncRequest {
        sync_run_id: SyncRunId::new("github-actions-fixture-run").unwrap(),
        connection: ConnectionInput {
            connection_id: next_infra_core::ConnectionId::new("github-fixture-connection").unwrap(),
            connector_type: ConnectorType::new("github").unwrap(),
            config: json!({"selected_repository_ids": ["10"]}),
            config_schema_version: SchemaVersion::new(1).unwrap(),
        },
        mode: SyncMode::Full,
        scope: context.scope,
        cursor: None,
        targeted_resources: Vec::new(),
    };
    let normalized = normalizer.normalize(&request, batch).unwrap();
    assert_eq!(normalized.resources.len(), 3);
    assert_eq!(normalized.relations.len(), 3);
    assert_eq!(normalized.redaction_report.secret_sentinels_detected, 0);
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
