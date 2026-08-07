use async_trait::async_trait;
use next_infra_connector_api::{
    ConnectionInput, ConnectorFailure, ReadConnector, SyncOutcome, SyncRequest,
};
use next_infra_connector_ssh::{
    ProbeId, ProbeOutcome, ProbeOutput, SshBatchOutput, SshCancellation, SshClock,
    SshConnectionConfigV1, SshConnector, SshProbeClient,
};
use next_infra_core::{
    Connection, ConnectionId, ConnectorHealth, ConnectorType, ErrorCode, Lifecycle, ResourceKind,
    SchemaVersion, Scope, StoreReader, StoreWriter, SyncMode, SyncRunId, SyncTrigger, Timestamp,
};
use next_infra_normalizer::{AttributeSchema, Normalizer, RelationSchema};
use next_infra_store::Store;
use next_infra_sync::{SyncEngine, SyncRunStart};
use std::{collections::BTreeSet, sync::Mutex};
use tempfile::TempDir;

const IDENTITY: &str = "9f7fd5e6-3bc8-4daa-ae6b-9dfdffb54743";

struct FakeClient {
    batches: Mutex<Vec<Result<SshBatchOutput, ConnectorFailure>>>,
}

impl FakeClient {
    fn new(batches: Vec<Result<SshBatchOutput, ConnectorFailure>>) -> Self {
        Self {
            batches: Mutex::new(batches.into_iter().rev().collect()),
        }
    }
}

#[async_trait]
impl SshProbeClient for FakeClient {
    async fn execute_batch(
        &self,
        _config: &SshConnectionConfigV1,
        _probes: &[ProbeId],
        _cancellation: &SshCancellation,
    ) -> Result<SshBatchOutput, ConnectorFailure> {
        self.batches.lock().unwrap().pop().unwrap()
    }
}

#[derive(Clone, Copy)]
struct FixedClock(i64);

impl SshClock for FixedClock {
    fn now(&self) -> Timestamp {
        Timestamp::from_unix_millis(self.0).unwrap()
    }
}

#[tokio::test]
async fn ssh_partial_replay_preserves_service_until_authoritative_missing_threshold() {
    let tempdir = TempDir::new().unwrap();
    let mut store = Store::open(&tempdir.path().join("data/next-infra.db")).unwrap();
    let connection = connection();
    store.upsert_connection(connection.clone()).unwrap();
    let mut engine = SyncEngine::new(store);
    let normalizer = normalizer();

    let first = request("ssh-replay-first");
    commit_sync(
        &mut engine,
        &normalizer,
        &connection,
        &first,
        connector(service_success(), 1)
            .sync(first.clone(), None)
            .await
            .unwrap(),
        1,
    );
    assert_eq!(resources(engine.writer().store()).len(), 4);

    let partial = request("ssh-replay-partial");
    let partial_outcome = connector(service_failure(), 2)
        .sync(partial.clone(), None)
        .await
        .unwrap();
    assert!(matches!(partial_outcome, SyncOutcome::Partial { .. }));
    commit_sync(
        &mut engine,
        &normalizer,
        &connection,
        &partial,
        partial_outcome,
        2,
    );
    assert_eq!(
        systemd_lifecycle(engine.writer().store()),
        Lifecycle::Active
    );

    for (index, run_id) in ["ssh-replay-missing-one", "ssh-replay-missing-two"]
        .into_iter()
        .enumerate()
    {
        let request = request(run_id);
        let outcome = connector(service_missing(), 3 + index as i64)
            .sync(request.clone(), None)
            .await
            .unwrap();
        assert!(matches!(outcome, SyncOutcome::Complete { .. }));
        commit_sync(
            &mut engine,
            &normalizer,
            &connection,
            &request,
            outcome,
            3 + index as i64,
        );
        assert_eq!(
            systemd_lifecycle(engine.writer().store()),
            if index == 0 {
                Lifecycle::Active
            } else {
                Lifecycle::Tombstoned
            }
        );
    }
}

fn connector(
    child: Result<SshBatchOutput, ConnectorFailure>,
    observed_at: i64,
) -> SshConnector<FakeClient, FixedClock> {
    SshConnector::with_clock(
        FakeClient::new(vec![
            batch(vec![success(ProbeId::HostIdentityV1, b"Linux\nx86_64\n")]),
            child,
        ]),
        FixedClock(observed_at),
    )
}

fn common(service: ProbeOutcome) -> Result<SshBatchOutput, ConnectorFailure> {
    batch(vec![
        success(ProbeId::HostUptimeV1, b"12:00 up 2 days, 1 user"),
        success(
            ProbeId::HostFilesystemsV1,
            b"Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/a 10 1 9 10% /a\n",
        ),
        success(ProbeId::HostProcessSummaryV1, b"R process\n"),
        service,
    ])
}

fn service_success() -> Result<SshBatchOutput, ConnectorFailure> {
    common(success(
        ProbeId::LinuxSystemdServicesV1,
        b"app.service loaded active running fixture\n",
    ))
}

fn service_missing() -> Result<SshBatchOutput, ConnectorFailure> {
    common(success(ProbeId::LinuxSystemdServicesV1, b""))
}

fn service_failure() -> Result<SshBatchOutput, ConnectorFailure> {
    common(ProbeOutcome::Failure {
        probe_id: ProbeId::LinuxSystemdServicesV1,
        failure: ConnectorFailure {
            code: ErrorCode::ProviderUnavailable,
            message: "SSH probe failed".into(),
            retryable: true,
            retry_after_ms: None,
        },
    })
}

fn success(probe_id: ProbeId, stdout: &'static [u8]) -> ProbeOutcome {
    ProbeOutcome::Success(ProbeOutput::from_collected_stdout(probe_id, stdout.to_vec(), 1).unwrap())
}

fn batch(outcomes: Vec<ProbeOutcome>) -> Result<SshBatchOutput, ConnectorFailure> {
    Ok(SshBatchOutput {
        outcomes,
        elapsed_ms: 5,
        output_bytes: 100,
    })
}

fn request(run_id: &str) -> SyncRequest {
    SyncRequest {
        sync_run_id: SyncRunId::new(run_id).unwrap(),
        connection: ConnectionInput {
            connection_id: ConnectionId::new("ssh-fixture-connection").unwrap(),
            connector_type: ConnectorType::new("ssh").unwrap(),
            config: config(),
            config_schema_version: SchemaVersion::new(1).unwrap(),
        },
        mode: SyncMode::Full,
        scope: Scope::new("ssh-fixture-scope").unwrap(),
        cursor: None,
        targeted_resources: Vec::new(),
    }
}

fn connection() -> Connection {
    Connection {
        connection_id: ConnectionId::new("ssh-fixture-connection").unwrap(),
        connector_type: ConnectorType::new("ssh").unwrap(),
        display_name: "SSH Fixture".into(),
        enabled: true,
        config: config(),
        secret_ref: None,
        health: ConnectorHealth::Healthy,
        last_success_at: None,
        last_attempt_at: None,
        config_schema_version: SchemaVersion::new(1).unwrap(),
        deleted_at: None,
    }
}

fn config() -> serde_json::Value {
    serde_json::json!({
        "host_identity": IDENTITY,
        "host_alias": "fixture-host",
        "connect_timeout_secs": 10,
        "probe_profile": "baseline-v1",
        "allowed_service_ids": ["app.service"],
    })
}

fn commit_sync(
    engine: &mut SyncEngine<Store>,
    normalizer: &Normalizer,
    connection: &Connection,
    request: &SyncRequest,
    outcome: SyncOutcome,
    timestamp: i64,
) {
    let handle = engine
        .start(
            connection,
            SyncRunStart {
                sync_run_id: request.sync_run_id.clone(),
                mode: request.mode,
                trigger: SyncTrigger::Schedule,
                scope: request.scope.clone(),
                started_at: Timestamp::from_unix_millis(timestamp).unwrap(),
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
            Timestamp::from_unix_millis(timestamp + 1).unwrap(),
        )
        .unwrap();
}

fn resources(store: &Store) -> Vec<next_infra_core::Resource> {
    store
        .list_resources_for_scope(
            &ConnectionId::new("ssh-fixture-connection").unwrap(),
            &Scope::new("ssh-fixture-scope").unwrap(),
        )
        .unwrap()
}

fn systemd_lifecycle(store: &Store) -> Lifecycle {
    resources(store)
        .into_iter()
        .find(|resource| resource.kind.as_str() == "ssh.systemd-service")
        .unwrap()
        .lifecycle
}

fn normalizer() -> Normalizer {
    Normalizer::new(
        [
            schema("ssh.host", &["platform", "architecture", "uptime_bucket"]),
            schema("ssh.filesystem", &["host_identity", "entries"]),
            schema("ssh.process-summary", &["host_identity", "total", "states"]),
            schema(
                "ssh.systemd-service",
                &["unit", "load_state", "active_state", "sub_state"],
            ),
        ],
        [
            relation("ssh.contains", "ssh.host", "ssh.filesystem"),
            relation("ssh.contains", "ssh.host", "ssh.process-summary"),
            relation("ssh.contains", "ssh.host", "ssh.systemd-service"),
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
