use async_trait::async_trait;
use next_infra_connector_api::{ConnectionInput, ReadConnector, SyncOutcome, SyncRequest};
use next_infra_connector_github::{
    GitHubClock, GitHubConnector, GitHubError, GitHubResponseHeaders, GitHubTransport,
    GitHubTransportRequest, GitHubTransportResponse,
};
use next_infra_core::{
    Connection, ConnectionId, ConnectorHealth, ConnectorType, Lifecycle, ResourceKind,
    SchemaVersion, Scope, SecretValue, StoreReader, StoreWriter, SyncMode, SyncRunId,
    SyncRunStatus, SyncTrigger, Timestamp,
};
use next_infra_normalizer::{AttributeSchema, Normalizer, RelationSchema};
use next_infra_store::Store;
use next_infra_sync::{SyncEngine, SyncRunStart};
use reqwest::StatusCode;
use std::{collections::BTreeSet, sync::Mutex};
use tempfile::TempDir;

struct FixedClock(u64);

impl GitHubClock for FixedClock {
    fn now_epoch_seconds(&self) -> u64 {
        self.0
    }
}

struct FakeTransport {
    responses: Mutex<Vec<Result<GitHubTransportResponse, GitHubError>>>,
}

impl FakeTransport {
    fn new(responses: Vec<Result<GitHubTransportResponse, GitHubError>>) -> Self {
        Self {
            responses: Mutex::new(responses.into_iter().rev().collect()),
        }
    }
}

#[async_trait]
impl GitHubTransport for FakeTransport {
    async fn execute(
        &self,
        request: GitHubTransportRequest,
    ) -> Result<GitHubTransportResponse, GitHubError> {
        assert!(request.authorization_is_sensitive());
        self.responses.lock().unwrap().pop().unwrap()
    }
}

#[tokio::test]
async fn github_partial_replay_commits_without_tombstoning_omitted_children() {
    let tempdir = TempDir::new().unwrap();
    let mut store = Store::open(&tempdir.path().join("data/next-infra.db")).unwrap();
    let connection = connection();
    store.upsert_connection(connection.clone()).unwrap();
    let mut engine = SyncEngine::new(store);
    let normalizer = normalizer();

    let first_request = request("github-pipeline-first");
    let first_connector =
        GitHubConnector::with_clock(FakeTransport::new(full_responses()), FixedClock(1_000));
    let first_outcome = first_connector
        .sync(
            first_request.clone(),
            Some(&SecretValue::new("fixture-token")),
        )
        .await
        .unwrap();
    let SyncOutcome::Complete { batch: first_batch } = &first_outcome else {
        panic!("GitHub first clean sync must be complete")
    };
    assert_eq!(first_batch.resources.len(), 3);
    commit(
        &mut engine,
        &normalizer,
        &connection,
        &first_request,
        first_outcome,
        1,
        2,
    );
    assert_eq!(resources(engine.writer().store()).len(), 3);

    let second_request = request("github-pipeline-second");
    let second_connector = GitHubConnector::with_clock(
        FakeTransport::new(repo_with_child_failures()),
        FixedClock(2_000),
    );
    let second_outcome = second_connector
        .sync(
            second_request.clone(),
            Some(&SecretValue::new("fixture-token")),
        )
        .await
        .unwrap();
    let SyncOutcome::Partial { batch, .. } = &second_outcome else {
        panic!("GitHub replay must remain partial")
    };
    assert_eq!(batch.resources.len(), 1);
    commit(
        &mut engine,
        &normalizer,
        &connection,
        &second_request,
        second_outcome,
        3,
        4,
    );

    let persisted = resources(engine.writer().store());
    assert_eq!(persisted.len(), 3);
    assert!(
        persisted
            .iter()
            .all(|resource| resource.lifecycle == Lifecycle::Active)
    );
    assert_eq!(
        engine
            .writer()
            .store()
            .get_sync_run(&second_request.sync_run_id)
            .unwrap()
            .unwrap()
            .status,
        SyncRunStatus::Partial
    );
}

fn commit(
    engine: &mut SyncEngine<Store>,
    normalizer: &Normalizer,
    connection: &Connection,
    request: &SyncRequest,
    outcome: SyncOutcome,
    started_at: i64,
    finished_at: i64,
) {
    let handle = engine
        .start(
            connection,
            SyncRunStart {
                sync_run_id: request.sync_run_id.clone(),
                mode: request.mode,
                trigger: SyncTrigger::Schedule,
                scope: request.scope.clone(),
                started_at: Timestamp::from_unix_millis(started_at).unwrap(),
                targeted_resources: Vec::new(),
            },
        )
        .unwrap();
    let normalized = normalizer
        .normalize(request, outcome.batch().clone())
        .unwrap();
    engine
        .commit(
            handle,
            normalized,
            Timestamp::from_unix_millis(finished_at).unwrap(),
        )
        .unwrap();
}

fn connection() -> Connection {
    Connection {
        connection_id: ConnectionId::new("github-fixture-connection").unwrap(),
        connector_type: ConnectorType::new("github").unwrap(),
        display_name: "GitHub Fixture".into(),
        enabled: true,
        config: serde_json::json!({"selected_repository_ids": ["10"]}),
        secret_ref: None,
        health: ConnectorHealth::Healthy,
        last_success_at: None,
        last_attempt_at: None,
        config_schema_version: SchemaVersion::new(1).unwrap(),
        deleted_at: None,
    }
}

fn request(run_id: &str) -> SyncRequest {
    SyncRequest {
        sync_run_id: SyncRunId::new(run_id).unwrap(),
        connection: ConnectionInput {
            connection_id: ConnectionId::new("github-fixture-connection").unwrap(),
            connector_type: ConnectorType::new("github").unwrap(),
            config: serde_json::json!({"selected_repository_ids": ["10"]}),
            config_schema_version: SchemaVersion::new(1).unwrap(),
        },
        mode: SyncMode::Full,
        scope: Scope::new("github-account-scope").unwrap(),
        cursor: None,
        targeted_resources: Vec::new(),
    }
}

fn resources(store: &Store) -> Vec<next_infra_core::Resource> {
    store
        .list_resources_for_scope(
            &ConnectionId::new("github-fixture-connection").unwrap(),
            &Scope::new("github-account-scope").unwrap(),
        )
        .unwrap()
}

fn response(
    status: StatusCode,
    body: &'static [u8],
) -> Result<GitHubTransportResponse, GitHubError> {
    Ok(GitHubTransportResponse::synthetic(
        status,
        GitHubResponseHeaders::default(),
        body,
    ))
}

fn full_responses() -> Vec<Result<GitHubTransportResponse, GitHubError>> {
    vec![
        response(StatusCode::OK, br#"[{"id":10,"name":"fixture-repo","owner":{"login":"fixture-owner"},"visibility":"private","default_branch":"main","archived":false,"disabled":false,"created_at":"2026-08-05T00:00:00Z","updated_at":"2026-08-05T00:01:00Z"}]"#),
        response(StatusCode::OK, br#"{"total_count":1,"workflows":[{"id":40,"name":"Fixture workflow","path":".github/workflows/fixture.yml","state":"active","created_at":"2026-08-05T00:00:00Z","updated_at":"2026-08-05T00:01:00Z"}]}"#),
        response(StatusCode::OK, br#"{"total_count":1,"workflow_runs":[{"id":50,"workflow_id":40,"name":"Fixture workflow","display_title":"Fixture run","run_number":1,"run_attempt":1,"event":"push","status":"completed","conclusion":"success","head_branch":"fixture-branch","created_at":"2026-08-05T00:00:00Z","updated_at":"2026-08-05T00:01:00Z","run_started_at":null}]}"#),
    ]
}

fn repo_with_child_failures() -> Vec<Result<GitHubTransportResponse, GitHubError>> {
    vec![
        full_responses().remove(0),
        response(StatusCode::FORBIDDEN, b"denied"),
        response(StatusCode::FORBIDDEN, b"denied"),
    ]
}

fn normalizer() -> Normalizer {
    Normalizer::new(
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
                "github.workflow",
                &["workflow_id", "path", "state", "created_at", "updated_at"],
            ),
            schema(
                "github.workflow_run",
                &[
                    "run_id",
                    "workflow_id",
                    "run_number",
                    "status",
                    "conclusion",
                    "created_at",
                ],
            ),
        ],
        [
            relation("github.contains", "github.repository", "github.workflow"),
            relation("github.executes", "github.workflow", "github.workflow_run"),
        ],
    )
    .unwrap()
}

fn schema(kind: &str, fields: &[&str]) -> AttributeSchema {
    AttributeSchema {
        kind: ResourceKind::new(kind).unwrap(),
        schema_version: SchemaVersion::new(1).unwrap(),
        allowed_attributes: fields
            .iter()
            .map(|field| (*field).into())
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
