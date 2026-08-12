use next_infra_binding::{BindingError, BindingInput, BindingService};
use next_infra_core::*;
use next_infra_query::{
    dto::{
        ChangeOriginDto, ChangeSubjectDto, EvidenceType, Lifecycle as DtoLifecycle,
        RelationEvidenceDto,
    },
    service::{GetTopologyRequest, QueryService, RecentChangesRequest},
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

fn sync_run(connection_id: &str, run_id: &str, started_at: i64, finished_at: i64) -> SyncRun {
    SyncRun {
        sync_run_id: id(run_id, SyncRunId::new),
        connection_id: id(connection_id, ConnectionId::new),
        mode: SyncMode::Full,
        trigger: SyncTrigger::User,
        started_at: timestamp(started_at),
        finished_at: Some(timestamp(finished_at)),
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

fn resource(
    resource_id: &str,
    connection_id: &str,
    run_id: &str,
    kind: &str,
    lifecycle: Lifecycle,
) -> Resource {
    Resource {
        resource_id: id(resource_id, ResourceId::new),
        connection_id: id(connection_id, ConnectionId::new),
        kind: id(kind, ResourceKind::new),
        external_id: id(format!("fixture-external-{resource_id}"), ExternalId::new),
        name: resource_id.to_owned(),
        display_name: format!("Fixture {resource_id}"),
        scope: id("fixture-scope", Scope::new),
        labels: Default::default(),
        lifecycle,
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

fn seed_resource(
    store: &mut Store,
    connection_id: &str,
    connector_type: &str,
    run_id: &str,
    resource_id: &str,
    kind: &str,
    lifecycle: Lifecycle,
) {
    store
        .upsert_connection(connection(connection_id, connector_type))
        .unwrap();
    let run = sync_run(connection_id, run_id, 1, 2);
    let mut running = run.clone();
    running.status = SyncRunStatus::Running;
    running.finished_at = None;
    store.start_sync_run(running).unwrap();
    store
        .commit_sync(SyncCommit {
            sync_run: run,
            resources: vec![resource(
                resource_id,
                connection_id,
                run_id,
                kind,
                lifecycle,
            )],
            resource_versions: Vec::new(),
            relations: Vec::new(),
            relation_versions: Vec::new(),
            changes: Vec::new(),
            cursor_after: None,
            missing_evidence: None,
        })
        .unwrap();
}

fn relation(
    relation_id: &str,
    source_resource_id: &str,
    target_resource_id: &str,
    kind: &str,
    evidence: RelationEvidence,
    lifecycle: Lifecycle,
) -> Relation {
    Relation {
        relation_id: id(relation_id, RelationId::new),
        source_resource_id: id(source_resource_id, ResourceId::new),
        target_resource_id: id(target_resource_id, ResourceId::new),
        kind: id(kind, RelationKind::new),
        evidence_key: id(format!("fixture-evidence-{relation_id}"), EvidenceKey::new),
        evidence,
        first_seen_at: timestamp(10),
        last_seen_at: timestamp(10),
        lifecycle,
    }
}

fn seed_provider_and_inferred_relations(store: &mut Store) -> (Relation, Relation) {
    let provider_run = sync_run("fixture-connection-source", "fixture-provider-run", 10, 11);
    let provider = relation(
        "fixture-provider-relation",
        "fixture-resource-source",
        "fixture-resource-target",
        "provider.links_to",
        RelationEvidence::Provider {
            connection_id: provider_run.connection_id.clone(),
            sync_run_id: provider_run.sync_run_id.clone(),
            field_path: id("fixture.attributes.target", FieldPath::new),
        },
        Lifecycle::Active,
    );
    let inferred = relation(
        "fixture-inferred-relation",
        "fixture-resource-target",
        "fixture-resource-third",
        "inferred.links_to",
        RelationEvidence::Inferred {
            rule_version: id("fixture-rule-v1", RuleVersion::new),
            input_resource_version_ids: vec![id(
                "fixture-input-resource-version",
                ResourceVersionId::new,
            )],
            input_relation_version_ids: vec![id(
                "fixture-input-relation-version",
                RelationVersionId::new,
            )],
            confidence: Confidence::from_basis_points(8_500).unwrap(),
        },
        Lifecycle::Active,
    );
    let mut running = provider_run.clone();
    running.status = SyncRunStatus::Running;
    running.finished_at = None;
    store.start_sync_run(running).unwrap();
    store
        .commit_sync(SyncCommit {
            sync_run: provider_run,
            resources: Vec::new(),
            resource_versions: Vec::new(),
            relations: vec![provider.clone(), inferred.clone()],
            relation_versions: Vec::new(),
            changes: Vec::new(),
            cursor_after: None,
            missing_evidence: None,
        })
        .unwrap();
    (provider, inferred)
}

fn binding_input(source_resource_id: &str, target_resource_id: &str, kind: &str) -> BindingInput {
    BindingInput {
        source_resource_id: id(source_resource_id, ResourceId::new),
        target_resource_id: id(target_resource_id, ResourceId::new),
        kind: id(kind, RelationKind::new),
    }
}

fn binding_store_error(error: BindingError<StoreError>) -> StoreError {
    StoreError::Contract(error.to_string())
}

fn query_context() -> QueryContextSnapshot {
    let schedules = [
        "fixture-connection-source",
        "fixture-connection-target",
        "fixture-connection-third",
    ]
    .into_iter()
    .map(|connection_id| {
        (
            id(connection_id, ConnectionId::new),
            QuerySchedule::new(1_000, None).unwrap(),
        )
    })
    .collect::<Vec<_>>();
    QueryContextSnapshot::new(timestamp(100), 1, schedules).unwrap()
}

fn assert_no_ipv4(value: &str) {
    assert!(
        !value
            .split(|character: char| { !character.is_ascii_digit() && character != '.' })
            .any(|candidate| {
                candidate.matches('.').count() == 3
                    && candidate
                        .split('.')
                        .all(|octet| !octet.is_empty() && octet.parse::<u8>().is_ok())
            }),
        "serialized output contains an IPv4 address: {value}"
    );
}

fn assert_serialized_safe(serialized: &str) {
    let lowercase = serialized.to_ascii_lowercase();
    for marker in [
        "http://",
        "https://",
        "://",
        "password",
        "secret",
        "token",
        "authorization",
        "bearer",
        "api_key",
        "private_key",
        "github.com",
        "supabase.co",
        "cloudflare.com",
        "dokploy.com",
        "tencentcloud.com",
    ] {
        assert!(
            !lowercase.contains(marker),
            "serialized output contains forbidden marker {marker}: {serialized}"
        );
    }
    assert_no_ipv4(serialized);
}

fn projected_relation(store: &Store, resource_id: &str, binding_id: &BindingId) -> Relation {
    let resources = BTreeSet::from([id(resource_id, ResourceId::new)]);
    store
        .query_relations_for_resources(&resources, 20, None)
        .unwrap()
        .body
        .items
        .into_iter()
        .find(|projected| {
            projected.relation.evidence
                == RelationEvidence::Configured {
                    binding_id: binding_id.clone(),
                }
        })
        .map(|projected| projected.relation)
        .expect("configured relation was not projected")
}

#[test]
fn manual_binding_mutations_keep_provenance_and_other_evidence_immutable() {
    let directory = TempDir::new().unwrap();
    let mut store = Store::open(&directory.path().join("topology.db")).unwrap();
    seed_resource(
        &mut store,
        "fixture-connection-source",
        "fixture-source",
        "fixture-source-run",
        "fixture-resource-source",
        "fixture.source",
        Lifecycle::Active,
    );
    seed_resource(
        &mut store,
        "fixture-connection-target",
        "fixture-target",
        "fixture-target-run",
        "fixture-resource-target",
        "fixture.target",
        Lifecycle::Active,
    );
    seed_resource(
        &mut store,
        "fixture-connection-third",
        "fixture-third",
        "fixture-third-run",
        "fixture-resource-third",
        "fixture.third",
        Lifecycle::Active,
    );
    let (provider_before, inferred_before) = seed_provider_and_inferred_relations(&mut store);

    let shared_store = SharedStore::new(store);
    let binding = shared_store
        .write(|store| {
            BindingService::new(store)
                .create(
                    binding_input(
                        "fixture-resource-source",
                        "fixture-resource-target",
                        "infra.depends_on",
                    ),
                    timestamp(20),
                )
                .map_err(binding_store_error)
        })
        .unwrap();
    assert_eq!(binding.status, BindingStatus::Active);

    let source = id("fixture-resource-source", ResourceId::new);
    let query = QueryService::new(CommittedQuerySource::new(
        shared_store.clone(),
        ConnectorCatalogSnapshot::default(),
        query_context(),
    ));
    let created_topology = query
        .get_topology(GetTopologyRequest {
            focus_resource_id: source.as_str().to_owned(),
            depth: Some(2),
            max_nodes: Some(20),
            max_edges: Some(20),
        })
        .unwrap();
    let configured_edge = created_topology
        .edges
        .iter()
        .find(|edge| edge.evidence_type == EvidenceType::Configured)
        .expect("configured edge missing after create");
    assert_eq!(configured_edge.lifecycle, DtoLifecycle::Active);
    match &configured_edge.evidence {
        RelationEvidenceDto::Configured { binding_id, .. } => {
            assert_eq!(binding_id, binding.binding_id.as_str());
        }
        evidence => panic!("unexpected configured evidence: {evidence:?}"),
    }
    let configured_json = serde_json::to_string(configured_edge).unwrap();
    assert!(!configured_json.contains("sync_run_id"));
    assert!(!configured_json.contains("field_path"));
    assert_serialized_safe(&configured_json);

    let updated = shared_store
        .write(|store| {
            BindingService::new(store)
                .update(
                    &binding.binding_id,
                    binding_input(
                        "fixture-resource-source",
                        "fixture-resource-third",
                        "automation.deploys_to",
                    ),
                    timestamp(30),
                )
                .map_err(binding_store_error)
        })
        .unwrap();
    assert_eq!(updated.status, BindingStatus::Active);
    let disabled = shared_store
        .write(|store| {
            BindingService::new(store)
                .disable(&binding.binding_id, timestamp(40))
                .map_err(binding_store_error)
        })
        .unwrap();
    assert_eq!(disabled.status, BindingStatus::Disabled);

    let changes = query
        .get_recent_changes(RecentChangesRequest {
            limit: Some(20),
            ..RecentChangesRequest::default()
        })
        .unwrap();
    assert_eq!(changes.items.len(), 3);
    for change in &changes.items {
        match (&change.subject, &change.origin) {
            (
                ChangeSubjectDto::Binding {
                    binding_id: subject,
                },
                ChangeOriginDto::Binding { binding_id: origin },
            ) => {
                assert_eq!(subject, &binding.binding_id.as_str());
                assert_eq!(origin, &binding.binding_id.as_str());
            }
            (subject, origin) => {
                panic!("mutation was not Binding-origin: {subject:?} / {origin:?}")
            }
        }
        assert!(!change.fields.is_empty());
        let serialized = serde_json::to_string(change).unwrap();
        assert!(!serialized.contains("sync_run_id"));
        assert!(!serialized.contains("field_path"));
        assert_serialized_safe(&serialized);
    }
    assert_serialized_safe(&serde_json::to_string(&changes).unwrap());

    let provider_after = shared_store
        .read(|store| store.get_relation(&provider_before.relation_id))
        .unwrap()
        .unwrap();
    let inferred_after = shared_store
        .read(|store| store.get_relation(&inferred_before.relation_id))
        .unwrap()
        .unwrap();
    assert_eq!(provider_after, provider_before);
    assert_eq!(inferred_after, inferred_before);

    let final_topology = query
        .get_topology(GetTopologyRequest {
            focus_resource_id: source.as_str().to_owned(),
            depth: Some(2),
            max_nodes: Some(20),
            max_edges: Some(20),
        })
        .unwrap();
    assert!(
        final_topology
            .edges
            .iter()
            .any(|edge| edge.evidence_type == EvidenceType::Provider)
    );
    assert!(
        final_topology
            .edges
            .iter()
            .any(|edge| edge.evidence_type == EvidenceType::Inferred)
    );
    assert_serialized_safe(&serde_json::to_string(&final_topology).unwrap());
}

#[test]
fn unresolved_binding_retains_configured_evidence_until_reconcile() {
    let directory = TempDir::new().unwrap();
    let mut store = Store::open(&directory.path().join("topology.db")).unwrap();
    seed_resource(
        &mut store,
        "fixture-connection-source",
        "fixture-source",
        "fixture-source-run",
        "fixture-resource-source",
        "fixture.source",
        Lifecycle::Active,
    );
    seed_resource(
        &mut store,
        "fixture-connection-target",
        "fixture-target",
        "fixture-target-tombstone-run",
        "fixture-resource-target",
        "fixture.target",
        Lifecycle::Tombstoned,
    );

    let binding = BindingService::new(&mut store)
        .create(
            binding_input(
                "fixture-resource-source",
                "fixture-resource-target",
                "network.routes_to",
            ),
            timestamp(20),
        )
        .unwrap();
    assert_eq!(binding.status, BindingStatus::Unresolved);
    let orphaned = projected_relation(&store, "fixture-resource-source", &binding.binding_id);
    assert_eq!(orphaned.lifecycle, Lifecycle::Orphaned);
    assert_eq!(
        orphaned.evidence,
        RelationEvidence::Configured {
            binding_id: binding.binding_id.clone(),
        }
    );
    assert_serialized_safe(&serde_json::to_string(&orphaned).unwrap());

    seed_resource(
        &mut store,
        "fixture-connection-target",
        "fixture-target",
        "fixture-target-active-run",
        "fixture-resource-target",
        "fixture.target",
        Lifecycle::Active,
    );
    let reconciled = BindingService::new(&mut store)
        .reconcile(&binding.binding_id, timestamp(40))
        .unwrap();
    assert_eq!(reconciled.status, BindingStatus::Active);
    let active = projected_relation(&store, "fixture-resource-source", &binding.binding_id);
    assert_eq!(active.relation_id, orphaned.relation_id);
    assert_eq!(active.lifecycle, Lifecycle::Active);
    assert_eq!(active.evidence, orphaned.evidence);
    assert_serialized_safe(&serde_json::to_string(&active).unwrap());
}
