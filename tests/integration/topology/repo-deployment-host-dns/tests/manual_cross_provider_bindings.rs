use next_infra_binding::{BindingError, BindingInput, BindingService};
use next_infra_core::*;
use next_infra_query::{
    dto::{EvidenceType, Lifecycle as DtoLifecycle, RelationEvidenceDto, TopologyDto},
    service::{GetTopologyRequest, QueryService},
};
use next_infra_runtime::{
    CommittedQuerySource, ConnectorCatalogSnapshot, QueryContextSnapshot, QuerySchedule,
    SharedStore,
};
use next_infra_store::{Store, StoreError};
use serde_json::json;
use std::collections::BTreeSet;
use tempfile::TempDir;

fn timestamp(value: i64) -> Timestamp {
    Timestamp::from_unix_millis(value).unwrap()
}

fn id<T>(value: impl Into<String>, build: impl FnOnce(String) -> Result<T, DomainError>) -> T {
    build(value.into()).unwrap()
}

fn connection(connection_id: &str, connector_type: &str) -> Connection {
    Connection {
        connection_id: id(connection_id, ConnectionId::new),
        connector_type: id(connector_type, ConnectorType::new),
        display_name: format!("Fixture {connector_type} connection"),
        enabled: true,
        config: json!({}),
        secret_ref: None,
        health: ConnectorHealth::Healthy,
        last_success_at: None,
        last_attempt_at: None,
        config_schema_version: SchemaVersion::new(1).unwrap(),
        deleted_at: None,
    }
}

fn sync_run(connection_id: &str, run_id: &str) -> SyncRun {
    SyncRun {
        sync_run_id: id(run_id, SyncRunId::new),
        connection_id: id(connection_id, ConnectionId::new),
        mode: SyncMode::Full,
        trigger: SyncTrigger::User,
        started_at: timestamp(1),
        finished_at: Some(timestamp(2)),
        status: SyncRunStatus::Succeeded,
        coverage: SyncCoverage::AuthoritativeFull {
            scope: id("fixture-scope", Scope::new),
        },
        cursor_before: None,
        cursor_after: None,
        counts: SyncRunCounts::default(),
        errors: Vec::new(),
        warnings: Vec::new(),
    }
}

fn resource(resource_id: &str, connection_id: &str, run_id: &str, kind: &str) -> Resource {
    Resource {
        resource_id: id(resource_id, ResourceId::new),
        connection_id: id(connection_id, ConnectionId::new),
        kind: id(kind, ResourceKind::new),
        external_id: id(format!("fixture-external-{resource_id}"), ExternalId::new),
        name: resource_id.to_owned(),
        display_name: format!("Fixture {resource_id}"),
        scope: id("fixture-scope", Scope::new),
        labels: Default::default(),
        lifecycle: Lifecycle::Active,
        health: ResourceHealth::Unknown,
        attributes: json!({}),
        attribute_schema_version: SchemaVersion::new(1).unwrap(),
        fingerprint: id(
            format!("fixture-fingerprint-{resource_id}"),
            Fingerprint::new,
        ),
        first_seen_at: timestamp(1),
        last_seen_at: timestamp(1),
        last_changed_at: timestamp(1),
        last_sync_run_id: id(run_id, SyncRunId::new),
    }
}

fn seed_connection(
    store: &mut Store,
    connection_id: &str,
    connector_type: &str,
    run_id: &str,
    resources: &[(&str, &str)],
) {
    store
        .upsert_connection(connection(connection_id, connector_type))
        .unwrap();
    let run = sync_run(connection_id, run_id);
    let mut running = run.clone();
    running.status = SyncRunStatus::Running;
    running.finished_at = None;
    store.start_sync_run(running).unwrap();
    store
        .commit_sync(SyncCommit {
            sync_run: run,
            resources: resources
                .iter()
                .map(|(resource_id, kind)| resource(resource_id, connection_id, run_id, kind))
                .collect(),
            resource_versions: Vec::new(),
            relations: Vec::new(),
            relation_versions: Vec::new(),
            changes: Vec::new(),
            cursor_after: None,
            missing_evidence: None,
        })
        .unwrap();
}

fn binding_input(source_resource_id: &str, target_resource_id: &str, kind: &str) -> BindingInput {
    BindingInput {
        source_resource_id: id(source_resource_id, ResourceId::new),
        target_resource_id: id(target_resource_id, ResourceId::new),
        kind: id(kind, RelationKind::new),
    }
}

fn query_context() -> QueryContextSnapshot {
    let connection_ids = [
        "fixture-connection-supabase-self-hosted",
        "fixture-connection-dokploy",
        "fixture-connection-tencent",
        "fixture-connection-ssh",
        "fixture-connection-github",
        "fixture-connection-cloudflare",
        "fixture-connection-supabase-managed",
    ];
    QueryContextSnapshot::new(
        timestamp(100),
        1,
        connection_ids.into_iter().map(|connection_id| {
            (
                id(connection_id, ConnectionId::new),
                QuerySchedule::new(1_000, None).unwrap(),
            )
        }),
    )
    .unwrap()
}

fn topology(
    store: &SharedStore,
    context: &QueryContextSnapshot,
    focus_resource_id: &str,
) -> TopologyDto {
    let source = CommittedQuerySource::new(
        store.clone(),
        ConnectorCatalogSnapshot::default(),
        context.clone(),
    );
    QueryService::new(source)
        .get_topology(GetTopologyRequest {
            focus_resource_id: focus_resource_id.to_owned(),
            depth: Some(1),
            max_nodes: Some(20),
            max_edges: Some(20),
        })
        .unwrap()
}

fn assert_configured_edge(
    topology: &TopologyDto,
    source_resource_id: &str,
    target_resource_id: &str,
    kind: &str,
    binding: &Binding,
) {
    let edge = topology
        .edges
        .iter()
        .find(|edge| {
            edge.source_resource_id == source_resource_id
                && edge.target_resource_id == target_resource_id
                && edge.kind == kind
                && edge.evidence_type == EvidenceType::Configured
        })
        .unwrap_or_else(|| {
            panic!(
                "configured edge missing: {source_resource_id} -> {kind} -> {target_resource_id}"
            )
        });
    assert_eq!(edge.lifecycle, DtoLifecycle::Active);
    match &edge.evidence {
        RelationEvidenceDto::Configured {
            binding_id,
            created_at,
        } => {
            assert_eq!(binding_id, binding.binding_id.as_str());
            assert!(!created_at.is_empty());
        }
        evidence => panic!("unexpected evidence for configured edge: {evidence:?}"),
    }
    assert!(
        !serde_json::to_string(&edge.evidence)
            .unwrap()
            .contains("sync_run_id")
    );

    let node_ids = topology
        .nodes
        .iter()
        .map(|node| node.resource_id.as_str())
        .collect::<BTreeSet<_>>();
    assert!(node_ids.contains(source_resource_id));
    assert!(node_ids.contains(target_resource_id));
    let source_node = topology
        .nodes
        .iter()
        .find(|node| node.resource_id == source_resource_id)
        .unwrap();
    let target_node = topology
        .nodes
        .iter()
        .find(|node| node.resource_id == target_resource_id)
        .unwrap();
    assert_ne!(source_node.resource_id, target_node.resource_id);
    assert_ne!(source_node.connection_id, target_node.connection_id);
}

#[test]
fn manual_cross_provider_bindings_replay_through_sqlite_and_query() {
    let directory = TempDir::new().unwrap();
    let mut store = Store::open(&directory.path().join("topology.db")).unwrap();

    seed_connection(
        &mut store,
        "fixture-connection-supabase-self-hosted",
        "supabase-self-hosted",
        "fixture-run-supabase-self-hosted",
        &[(
            "fixture-resource-supabase-instance",
            "supabase.self_hosted.instance",
        )],
    );
    seed_connection(
        &mut store,
        "fixture-connection-dokploy",
        "dokploy",
        "fixture-run-dokploy",
        &[
            (
                "fixture-resource-dokploy-application",
                "dokploy.application",
            ),
            ("fixture-resource-dokploy-domain", "dokploy.domain"),
        ],
    );
    seed_connection(
        &mut store,
        "fixture-connection-tencent",
        "tencent",
        "fixture-run-tencent",
        &[("fixture-resource-tencent-cvm", "tencent.cvm.instance")],
    );
    seed_connection(
        &mut store,
        "fixture-connection-ssh",
        "ssh",
        "fixture-run-ssh",
        &[("fixture-resource-ssh-host", "ssh.host")],
    );
    seed_connection(
        &mut store,
        "fixture-connection-github",
        "github",
        "fixture-run-github",
        &[("fixture-resource-github-workflow", "github.workflow")],
    );
    seed_connection(
        &mut store,
        "fixture-connection-cloudflare",
        "cloudflare",
        "fixture-run-cloudflare",
        &[("fixture-resource-cloudflare-dns", "cloudflare.dns_record")],
    );
    seed_connection(
        &mut store,
        "fixture-connection-supabase-managed",
        "supabase-managed",
        "fixture-run-supabase-managed",
        &[(
            "fixture-resource-supabase-managed-project",
            "supabase.managed.project",
        )],
    );

    let binding_specs = [
        (
            "fixture-resource-supabase-instance",
            "fixture-resource-dokploy-application",
            "infra.deployed_via",
            10,
        ),
        (
            "fixture-resource-tencent-cvm",
            "fixture-resource-ssh-host",
            "infra.accessed_via",
            11,
        ),
        (
            "fixture-resource-github-workflow",
            "fixture-resource-dokploy-application",
            "automation.deploys_to",
            12,
        ),
        (
            "fixture-resource-cloudflare-dns",
            "fixture-resource-dokploy-domain",
            "network.routes_to",
            13,
        ),
        (
            "fixture-resource-github-workflow",
            "fixture-resource-supabase-managed-project",
            "data.writes_to",
            14,
        ),
    ];
    let mut bindings = Vec::new();
    for (source, target, kind, at) in binding_specs {
        let binding = BindingService::new(&mut store)
            .create(binding_input(source, target, kind), timestamp(at))
            .unwrap();
        assert_eq!(binding.status, BindingStatus::Active);
        bindings.push(binding);
    }

    let duplicate = BindingService::new(&mut store).create(
        binding_input(
            "fixture-resource-tencent-cvm",
            "fixture-resource-ssh-host",
            "infra.accessed_via",
        ),
        timestamp(20),
    );
    assert!(matches!(duplicate, Err(BindingError::Duplicate)));

    let shared_store = SharedStore::new(store);
    let context = query_context();
    let supabase_topology = topology(
        &shared_store,
        &context,
        "fixture-resource-supabase-instance",
    );
    assert_configured_edge(
        &supabase_topology,
        "fixture-resource-supabase-instance",
        "fixture-resource-dokploy-application",
        "infra.deployed_via",
        &bindings[0],
    );

    let tencent_topology = topology(&shared_store, &context, "fixture-resource-tencent-cvm");
    assert_configured_edge(
        &tencent_topology,
        "fixture-resource-tencent-cvm",
        "fixture-resource-ssh-host",
        "infra.accessed_via",
        &bindings[1],
    );

    let workflow_topology = topology(&shared_store, &context, "fixture-resource-github-workflow");
    assert_configured_edge(
        &workflow_topology,
        "fixture-resource-github-workflow",
        "fixture-resource-dokploy-application",
        "automation.deploys_to",
        &bindings[2],
    );
    assert_configured_edge(
        &workflow_topology,
        "fixture-resource-github-workflow",
        "fixture-resource-supabase-managed-project",
        "data.writes_to",
        &bindings[4],
    );
    let workflow_node_ids = workflow_topology
        .nodes
        .iter()
        .map(|node| node.resource_id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(workflow_node_ids.len(), 3);

    let cloudflare_topology = topology(&shared_store, &context, "fixture-resource-cloudflare-dns");
    assert_configured_edge(
        &cloudflare_topology,
        "fixture-resource-cloudflare-dns",
        "fixture-resource-dokploy-domain",
        "network.routes_to",
        &bindings[3],
    );

    let disabled_binding_id = bindings[1].binding_id.clone();
    let disabled = shared_store
        .write(|store| {
            BindingService::new(store)
                .disable(&disabled_binding_id, timestamp(30))
                .map_err(|error| StoreError::Contract(error.to_string()))
        })
        .unwrap();
    assert_eq!(disabled.status, BindingStatus::Disabled);
    let persisted = shared_store
        .read(|store| store.get_binding(&disabled_binding_id))
        .unwrap()
        .unwrap();
    assert_eq!(persisted.status, BindingStatus::Disabled);

    let disabled_topology = topology(&shared_store, &context, "fixture-resource-tencent-cvm");
    let disabled_edges = disabled_topology
        .edges
        .iter()
        .filter(|edge| match &edge.evidence {
            RelationEvidenceDto::Configured { binding_id, .. } => {
                binding_id == disabled_binding_id.as_str()
            }
            _ => false,
        })
        .collect::<Vec<_>>();
    assert!(
        disabled_edges
            .iter()
            .all(|edge| edge.lifecycle != DtoLifecycle::Active)
    );
}
