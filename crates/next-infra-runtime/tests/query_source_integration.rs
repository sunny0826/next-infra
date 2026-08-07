use next_infra_core::{
    BindingId, Change, ChangeId, ChangeSubject, Connection, ConnectionId, ConnectorHealth,
    ConnectorType, DomainError, EvidenceKey, ExternalId, FieldChange, FieldPath, Fingerprint,
    Lifecycle, OriginRef, Relation, RelationEvidence, RelationId, RelationKind, RelationVersion,
    RelationVersionId, Resource, ResourceHealth, ResourceId, ResourceKind, ResourceVersion,
    ResourceVersionId, RuleVersion, SchemaVersion, Scope, SecretBackend, SecretKind, SecretRef,
    SecretRefInput, StoreWriter, SyncCommit, SyncCoverage, SyncMode, SyncRun, SyncRunCounts,
    SyncRunId, SyncRunStatus, SyncTrigger, Timestamp,
};
use next_infra_query::dto::{
    ConnectorCoverageLevelDto, EvidenceType, Freshness, RelationEvidenceDto, TimelineItemDto,
    TimelineOriginDto, TimelineVersionLinkDto,
};
use next_infra_query::service::{
    GetResourceRequest, GetTopologyRequest, QueryService, ResourceInclude, SearchResourcesRequest,
    SyncStatusRequest, TimelineRequest,
};
use next_infra_runtime::{
    CommittedQuerySource, ConnectorCatalogSnapshot, QueryContextSnapshot, QuerySchedule,
    SharedStore, SqliteRuntimeBackend,
};
use next_infra_store::Store;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use tempfile::TempDir;

fn timestamp(value: i64) -> Timestamp {
    Timestamp::from_unix_millis(value).unwrap()
}

fn id<T>(
    value: impl Into<String>,
    constructor: impl FnOnce(String) -> Result<T, DomainError>,
) -> T {
    constructor(value.into()).unwrap()
}

fn connection() -> Connection {
    Connection {
        connection_id: id("fixture-connection", ConnectionId::new),
        connector_type: ConnectorType::new("fixture").unwrap(),
        display_name: "Fixture connection".into(),
        enabled: true,
        config: json!({"api_token": "must-not-escape"}),
        secret_ref: Some(
            SecretRef::new(SecretRefInput {
                backend: SecretBackend::MacosDataProtectionKeychainV1,
                service: "dev.example.next-infra.provider-secret.v1".into(),
                account:
                    "connection/fixture-connection/kind/api-token/generation/fixture-generation"
                        .into(),
                secret_kind: SecretKind::ApiToken,
                generation_id: "fixture-generation".into(),
                created_at: timestamp(1),
                last_verified_at: timestamp(2),
                permission_scope_summary: "fixture read-only scope".into(),
            })
            .unwrap(),
        ),
        health: ConnectorHealth::Healthy,
        last_success_at: Some(timestamp(900)),
        last_attempt_at: Some(timestamp(950)),
        config_schema_version: SchemaVersion::new(1).unwrap(),
        deleted_at: None,
    }
}

fn run(status: SyncRunStatus, run_id: &str) -> SyncRun {
    SyncRun {
        sync_run_id: id(run_id, SyncRunId::new),
        connection_id: id("fixture-connection", ConnectionId::new),
        mode: SyncMode::Full,
        trigger: SyncTrigger::User,
        started_at: timestamp(900),
        finished_at: (status != SyncRunStatus::Running).then(|| timestamp(1_100)),
        status,
        coverage: SyncCoverage::AuthoritativeFull {
            scope: id("fixture-scope", Scope::new),
        },
        cursor_before: None,
        cursor_after: Some(id("cursor-v1", next_infra_core::SyncCursor::new)),
        counts: SyncRunCounts::default(),
        errors: Vec::new(),
    }
}

fn resource(resource_id: &str, observed_at: i64, run_id: &str) -> Resource {
    Resource {
        resource_id: id(resource_id, ResourceId::new),
        connection_id: id("fixture-connection", ConnectionId::new),
        kind: ResourceKind::new("fixture.resource").unwrap(),
        external_id: id(format!("external-{resource_id}"), ExternalId::new),
        name: resource_id.into(),
        display_name: format!("Fixture {resource_id}"),
        scope: id("fixture-scope", Scope::new),
        labels: BTreeMap::from([(
            id("fixture.group", next_infra_core::LabelKey::new),
            "integration".into(),
        )]),
        lifecycle: Lifecycle::Active,
        health: ResourceHealth::Healthy,
        attributes: json!({"state": "ready"}),
        attribute_schema_version: SchemaVersion::new(1).unwrap(),
        fingerprint: id(format!("fingerprint-{resource_id}"), Fingerprint::new),
        first_seen_at: timestamp(observed_at),
        last_seen_at: timestamp(observed_at),
        last_changed_at: timestamp(observed_at),
        last_sync_run_id: id(run_id, SyncRunId::new),
    }
}

fn provider_relation(
    relation_id: &str,
    source_resource_id: &str,
    target_resource_id: &str,
    run_id: &str,
) -> Relation {
    Relation {
        relation_id: id(relation_id, RelationId::new),
        source_resource_id: id(source_resource_id, ResourceId::new),
        target_resource_id: id(target_resource_id, ResourceId::new),
        kind: RelationKind::new("fixture.depends_on").unwrap(),
        evidence_key: id(format!("evidence-{relation_id}"), EvidenceKey::new),
        evidence: RelationEvidence::Provider {
            connection_id: id("fixture-connection", ConnectionId::new),
            sync_run_id: id(run_id, SyncRunId::new),
            field_path: id("attributes.target", FieldPath::new),
        },
        first_seen_at: timestamp(1_000),
        last_seen_at: timestamp(1_000),
        lifecycle: Lifecycle::Active,
    }
}

fn change(change_id: &str, resource_id: &str, run_id: &str) -> Change {
    Change {
        change_id: id(change_id, ChangeId::new),
        subject: ChangeSubject::Resource {
            resource_id: id(resource_id, ResourceId::new),
        },
        observed_at: timestamp(1_000),
        fields: vec![FieldChange {
            path: id("attributes.state", FieldPath::new),
            before: Some(json!("pending")),
            after: Some(json!("ready")),
        }],
        origin: OriginRef::SyncRun {
            sync_run_id: id(run_id, SyncRunId::new),
        },
    }
}

fn fixture_catalog() -> ConnectorCatalogSnapshot {
    let snapshot: next_infra_connector_catalog::ConnectorCoverageSnapshot =
        serde_json::from_value(json!({
            "connector_type": "fixture",
            "connector_version": "1.0.0",
            "auth": {"kind": "none", "minimum_permissions": []},
            "sync_modes": ["full"],
            "rate_limit": {
                "default_max_concurrency": 1,
                "requests_per_minute": null,
                "respects_retry_after": true
            },
            "known_gaps": [],
            "modules": [
                {
                    "module": "fixture.relations",
                    "level": "partial",
                    "reason": "relation pagination",
                    "subject": {
                        "type": "relation",
                        "kind": "fixture.depends_on",
                        "source_kind": "fixture.resource",
                        "target_kind": "fixture.resource"
                    }
                },
                {
                    "module": "fixture.resources",
                    "level": "supported",
                    "reason": null,
                    "subject": {
                        "type": "resource",
                        "kind": "fixture.resource",
                        "attribute_schema_version": 1
                    }
                }
            ]
        }))
        .unwrap();
    ConnectorCatalogSnapshot::new([snapshot])
}

fn context(evaluated_at: i64, revision: u64, next_scheduled_at: i64) -> QueryContextSnapshot {
    QueryContextSnapshot::new(
        timestamp(evaluated_at),
        revision,
        [(
            id("fixture-connection", ConnectionId::new),
            QuerySchedule::new(1_000, Some(timestamp(next_scheduled_at))).unwrap(),
        )],
    )
    .unwrap()
}

fn committed_store(
    resources: Vec<Resource>,
    relations: Vec<Relation>,
    changes: Vec<Change>,
) -> (TempDir, SharedStore) {
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("data").join("runtime-query.db");
    let mut store = Store::open(&database).unwrap();
    store.upsert_connection(connection()).unwrap();
    let run_id = resources
        .first()
        .map(|resource| resource.last_sync_run_id.as_str())
        .or_else(|| {
            relations
                .first()
                .and_then(|relation| relation.last_sync_run_id().map(|id| id.as_str()))
        })
        .unwrap_or("fixture-run");
    store
        .start_sync_run(run(SyncRunStatus::Running, run_id))
        .unwrap();
    store
        .commit_sync(SyncCommit {
            sync_run: run(SyncRunStatus::Succeeded, run_id),
            resources,
            resource_versions: Vec::new(),
            relations,
            relation_versions: Vec::new(),
            changes,
            cursor_after: Some(id("cursor-v1", next_infra_core::SyncCursor::new)),
            missing_evidence: None,
        })
        .unwrap();
    (directory, SharedStore::new(store))
}

fn service(
    store: SharedStore,
    evaluated_at: i64,
    revision: u64,
) -> QueryService<CommittedQuerySource> {
    QueryService::new(CommittedQuerySource::new(
        store,
        fixture_catalog(),
        context(evaluated_at, revision, evaluated_at + 500),
    ))
}

#[test]
fn shared_store_open_creates_one_database_and_clones_share_backend_query_projection() {
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("data").join("runtime-query.db");
    let shared_store = SharedStore::open(&database).unwrap();
    assert!(database.is_file());

    let source = CommittedQuerySource::new(
        shared_store.clone(),
        fixture_catalog(),
        context(2_000, 1, 2_500),
    );
    let service = QueryService::new(source);
    let mut backend = SqliteRuntimeBackend::from_shared_store(shared_store.clone());

    backend
        .sync_engine_mut()
        .writer_mut()
        .store_mut()
        .upsert_connection(connection())
        .unwrap();

    let connections = service.list_connections().unwrap();
    assert_eq!(connections.items.len(), 1);
    assert_eq!(connections.items[0].connection_id, "fixture-connection");

    let committed_revision = backend
        .shared_store()
        .read(Store::projection_metadata)
        .unwrap()
        .committed_revision;
    assert!(committed_revision > 0);
    assert!(
        connections
            .metadata
            .snapshot_version
            .starts_with(&format!("nis1:{committed_revision}:"))
    );
}

#[test]
fn committed_query_service_keeps_snapshot_metadata_and_safe_projections_together() {
    let run_id = "fixture-run";
    let resources = vec![
        resource("fixture-focus", 1_000, run_id),
        resource("fixture-target", 1_000, run_id),
    ];
    let relations = vec![provider_relation(
        "fixture-relation",
        "fixture-focus",
        "fixture-target",
        run_id,
    )];
    let changes = vec![change("fixture-change", "fixture-focus", run_id)];
    let (_directory, mut shared_store) = committed_store(resources, relations, changes);
    shared_store
        .start_sync_run(run(SyncRunStatus::Running, "fixture-failed-run"))
        .unwrap();
    let mut failed_run = run(SyncRunStatus::Failed, "fixture-failed-run");
    failed_run.cursor_after = None;
    failed_run.errors.push(DomainError {
        code: next_infra_core::ErrorCode::ProviderUnavailable,
        message: "provider token=must-not-escape at /private/runtime.db".into(),
        retryable: true,
    });
    shared_store
        .commit_sync(SyncCommit {
            sync_run: failed_run,
            resources: Vec::new(),
            resource_versions: Vec::new(),
            relations: Vec::new(),
            relation_versions: Vec::new(),
            changes: Vec::new(),
            cursor_after: None,
            missing_evidence: None,
        })
        .unwrap();
    let service = service(shared_store, 2_000, 1);

    let search = service
        .search_resources(SearchResourcesRequest {
            limit: Some(10),
            ..SearchResourcesRequest::default()
        })
        .unwrap();
    let detail = service
        .get_resource(GetResourceRequest {
            resource_id: "fixture-focus".into(),
            include: BTreeSet::from([
                ResourceInclude::Attributes,
                ResourceInclude::Relations,
                ResourceInclude::RecentChanges,
                ResourceInclude::ConnectorCoverage,
            ]),
        })
        .unwrap();
    let changes = service.get_recent_changes(Default::default()).unwrap();
    let status = service
        .get_sync_status(SyncStatusRequest {
            connection_id: "fixture-connection".into(),
            recent_run_limit: Some(10),
        })
        .unwrap();
    let coverage = service.list_connector_coverage().unwrap();
    let connections = service.list_connections().unwrap();

    let snapshot_versions = [
        search.metadata.snapshot_version.as_str(),
        detail.metadata.snapshot_version.as_str(),
        changes.metadata.snapshot_version.as_str(),
        status.metadata.snapshot_version.as_str(),
        coverage.metadata.snapshot_version.as_str(),
        connections.metadata.snapshot_version.as_str(),
    ];
    assert!(snapshot_versions.windows(2).all(|pair| pair[0] == pair[1]));
    assert_eq!(detail.metadata.generated_at, "1970-01-01T00:00:02.000Z");
    assert_eq!(search.items.len(), 2);
    assert_eq!(detail.resource.resource_id, "fixture-focus");
    assert_eq!(detail.resource.freshness, Freshness::Fresh);
    assert_eq!(detail.relations.len(), 1);
    assert_eq!(detail.recent_changes.len(), 1);
    assert_eq!(detail.connector_coverage.len(), 2);
    assert_eq!(
        detail.connector_coverage[0].level,
        ConnectorCoverageLevelDto::Partial
    );
    assert_eq!(coverage.items.len(), 2);
    assert_eq!(coverage.items[0].module, "fixture.relations");
    assert_eq!(coverage.items[0].level, ConnectorCoverageLevelDto::Partial);
    assert_eq!(coverage.items[1].module, "fixture.resources");
    assert_eq!(
        coverage.items[1].level,
        ConnectorCoverageLevelDto::Supported
    );
    assert!(matches!(
        detail.relations[0].evidence,
        RelationEvidenceDto::Provider {
            ref connector_type,
            ref connection_id,
            ..
        } if connector_type == "fixture" && connection_id == "fixture-connection"
    ));
    assert_eq!(
        status.next_scheduled_at.as_deref(),
        Some("1970-01-01T00:00:02.500Z")
    );
    let failed_error = status
        .recent_runs
        .iter()
        .find(|run| run.sync_run_id == "fixture-failed-run")
        .and_then(|run| run.errors.first())
        .unwrap();
    assert_eq!(failed_error.code, "provider_unavailable");
    assert_eq!(failed_error.message, "The provider was unavailable.");
    assert!(failed_error.retryable);

    let connection_json = serde_json::to_string(&connections).unwrap();
    assert!(!connection_json.contains("must-not-escape"));
    assert!(!connection_json.contains("fixture-generation"));
    assert!(!connection_json.contains("secret_ref"));
}

#[test]
fn freshness_uses_schedule_context_and_does_not_mutate_resource_health() {
    let run_id = "fixture-run";
    let mut saved_resource = resource("fixture-focus", 1_000, run_id);
    saved_resource.health = ResourceHealth::Degraded;
    let (_directory, shared_store) = committed_store(vec![saved_resource], Vec::new(), Vec::new());
    let source =
        CommittedQuerySource::new(shared_store, fixture_catalog(), context(2_000, 1, 2_500));
    let service = QueryService::new(source.clone());

    let fresh = service
        .search_resources(SearchResourcesRequest::default())
        .unwrap();
    assert_eq!(fresh.items[0].freshness, Freshness::Fresh);

    source.refresh_context(context(3_001, 2, 3_500)).unwrap();
    let stale = service
        .search_resources(SearchResourcesRequest::default())
        .unwrap();
    assert_eq!(stale.items[0].freshness, Freshness::Stale);

    source.refresh_context(context(4_001, 3, 4_500)).unwrap();
    let expired = service
        .search_resources(SearchResourcesRequest::default())
        .unwrap();
    assert_eq!(expired.items[0].freshness, Freshness::Expired);
    assert_ne!(
        fresh.metadata.snapshot_version,
        expired.metadata.snapshot_version
    );

    let health = service.get_health_summary().unwrap();
    assert_eq!(health.resource_health.degraded, 1);
    assert_eq!(health.freshness.expired, 1);
}

#[test]
fn topology_is_bounded_and_reports_frontier_and_provider_evidence() {
    let run_id = "fixture-run";
    let resources = [
        resource("fixture-focus", 1_000, run_id),
        resource("fixture-neighbor", 1_000, run_id),
        resource("fixture-incoming", 1_000, run_id),
    ]
    .into_iter()
    .collect();
    let relations = vec![
        provider_relation("a-focus-out", "fixture-focus", "fixture-neighbor", run_id),
        provider_relation("b-focus-in", "fixture-incoming", "fixture-focus", run_id),
    ];
    let (_directory, shared_store) = committed_store(resources, relations, Vec::new());
    let service = service(shared_store, 2_000, 1);

    let topology = service
        .get_topology(GetTopologyRequest {
            focus_resource_id: "fixture-focus".into(),
            depth: Some(1),
            max_nodes: Some(2),
            max_edges: Some(2),
        })
        .unwrap();

    assert!(topology.nodes.len() <= 2);
    assert!(topology.edges.len() <= 2);
    assert!(topology.truncated);
    assert!(!topology.frontier.is_empty());
    let node_ids = topology
        .nodes
        .iter()
        .map(|node| node.resource_id.as_str())
        .collect::<BTreeSet<_>>();
    assert!(topology.edges.iter().all(|edge| {
        node_ids.contains(edge.source_resource_id.as_str())
            && node_ids.contains(edge.target_resource_id.as_str())
    }));
    assert!(
        topology
            .edges
            .iter()
            .all(|edge| edge.evidence_type == EvidenceType::Provider)
    );
    assert!(topology.frontier.iter().any(|frontier| {
        frontier.resource_id == "fixture-incoming" || frontier.resource_id == "fixture-neighbor"
    }));
}

#[test]
fn resource_detail_rejects_store_truncation_instead_of_returning_partial_relations() {
    let run_id = "fixture-run";
    let mut resources = vec![resource("fixture-focus", 1_000, run_id)];
    let mut relations = Vec::new();
    for index in 0..=next_infra_store::MAX_DETAIL_RELATIONS {
        let target_id = format!("fixture-target-{index:03}");
        resources.push(resource(&target_id, 1_000, run_id));
        relations.push(provider_relation(
            &format!("fixture-relation-{index:03}"),
            "fixture-focus",
            &target_id,
            run_id,
        ));
    }
    let (_directory, shared_store) = committed_store(resources, relations, Vec::new());
    let service = service(shared_store, 2_000, 1);

    let error = service
        .get_resource(GetResourceRequest {
            resource_id: "fixture-focus".into(),
            include: BTreeSet::from([ResourceInclude::Relations]),
        })
        .unwrap_err();
    assert_eq!(error.code, "query_source_unavailable");
    assert!(!error.message.contains("truncated"));
    assert!(!error.message.contains("SQL"));
}

#[test]
fn timeline_groups_origins_links_versions_and_paginates_without_duplicates() {
    let run_id = "fixture-run";
    let resources = vec![
        resource("fixture-focus", 1_000, run_id),
        resource("fixture-target", 1_000, run_id),
    ];
    let relations = vec![
        provider_relation(
            "fixture-relation",
            "fixture-focus",
            "fixture-target",
            run_id,
        ),
        provider_relation(
            "fixture-binding-relation",
            "fixture-focus",
            "fixture-target",
            run_id,
        ),
    ];
    let resource_version = ResourceVersion {
        version_id: id("fixture-resource-version", ResourceVersionId::new),
        resource_id: id("fixture-focus", ResourceId::new),
        observed_at: timestamp(1_000),
        sync_run_id: id(run_id, SyncRunId::new),
        normalized_snapshot: json!({"state": "ready"}),
        fingerprint: id("fingerprint-resource-version", Fingerprint::new),
        schema_version: SchemaVersion::new(1).unwrap(),
        change_summary: vec![],
    };
    let sync_run_origin = OriginRef::SyncRun {
        sync_run_id: id(run_id, SyncRunId::new),
    };
    let relation_version = RelationVersion {
        relation_version_id: id("fixture-relation-version", RelationVersionId::new),
        relation_id: id("fixture-relation", RelationId::new),
        observed_at: timestamp(1_000),
        normalized_snapshot: json!({"kind": "fixture.depends_on"}),
        fingerprint: id("fingerprint-relation-version", Fingerprint::new),
        schema_version: SchemaVersion::new(1).unwrap(),
        origin: sync_run_origin.clone(),
    };
    let binding_origin = OriginRef::Binding {
        binding_id: id("fixture-binding", BindingId::new),
    };
    let binding_relation_version = RelationVersion {
        relation_version_id: id("fixture-binding-relation-version", RelationVersionId::new),
        relation_id: id("fixture-binding-relation", RelationId::new),
        observed_at: timestamp(2_000),
        normalized_snapshot: json!({"kind": "fixture.depends_on"}),
        fingerprint: id("fingerprint-binding-relation-version", Fingerprint::new),
        schema_version: SchemaVersion::new(1).unwrap(),
        origin: binding_origin.clone(),
    };
    let changes = vec![
        Change {
            change_id: id("fixture-change-resource", ChangeId::new),
            subject: ChangeSubject::Resource {
                resource_id: id("fixture-focus", ResourceId::new),
            },
            observed_at: timestamp(1_000),
            fields: vec![],
            origin: sync_run_origin.clone(),
        },
        Change {
            change_id: id("fixture-change-relation", ChangeId::new),
            subject: ChangeSubject::Relation {
                relation_id: id("fixture-relation", RelationId::new),
            },
            observed_at: timestamp(1_000),
            fields: vec![],
            origin: sync_run_origin,
        },
        Change {
            change_id: id("fixture-change-binding", ChangeId::new),
            subject: ChangeSubject::Binding {
                binding_id: id("fixture-binding", BindingId::new),
            },
            observed_at: timestamp(2_000),
            fields: vec![],
            origin: binding_origin,
        },
        Change {
            change_id: id("fixture-change-inference", ChangeId::new),
            subject: ChangeSubject::Resource {
                resource_id: id("fixture-focus", ResourceId::new),
            },
            observed_at: timestamp(3_000),
            fields: vec![],
            origin: OriginRef::Inference {
                rule_version: id("fixture-rule-v1", RuleVersion::new),
                input_resource_version_ids: vec![id(
                    "fixture-resource-version",
                    ResourceVersionId::new,
                )],
                input_relation_version_ids: vec![id(
                    "fixture-relation-version",
                    RelationVersionId::new,
                )],
            },
        },
    ];

    let directory = TempDir::new().unwrap();
    let database = directory.path().join("data").join("runtime-query.db");
    let mut store = Store::open(&database).unwrap();
    store.upsert_connection(connection()).unwrap();
    store
        .start_sync_run(run(SyncRunStatus::Running, run_id))
        .unwrap();
    store
        .commit_sync(SyncCommit {
            sync_run: run(SyncRunStatus::Succeeded, run_id),
            resources,
            resource_versions: vec![resource_version],
            relations,
            relation_versions: vec![relation_version, binding_relation_version],
            changes,
            cursor_after: Some(id("cursor-v1", next_infra_core::SyncCursor::new)),
            missing_evidence: None,
        })
        .unwrap();
    let service = service(SharedStore::new(store), 3_000, 1);

    let page = service
        .get_timeline(TimelineRequest {
            limit: None,
            cursor: None,
        })
        .unwrap();
    assert_eq!(page.metadata.generated_at, "1970-01-01T00:00:03.000Z");
    assert_eq!(page.groups.len(), 3);
    assert_eq!(
        &page.groups[0].origin,
        &TimelineOriginDto::Inference {
            rule_version: "fixture-rule-v1".into(),
            input_resource_version_ids: vec!["fixture-resource-version".into()],
            input_relation_version_ids: vec!["fixture-relation-version".into()],
        }
    );
    assert_eq!(
        &page.groups[1].origin,
        &TimelineOriginDto::Binding {
            binding_id: "fixture-binding".into(),
        }
    );
    assert_eq!(
        &page.groups[2].origin,
        &TimelineOriginDto::SyncRun {
            sync_run_id: "fixture-run".into(),
        }
    );
    assert_eq!(page.groups[2].items.len(), 2);

    let find_item = |change_id: &str| -> TimelineItemDto {
        page.groups
            .iter()
            .flat_map(|group| group.items.iter())
            .find(|item| item.change.change_id == change_id)
            .unwrap_or_else(|| panic!("missing timeline item {change_id}"))
            .clone()
    };
    let resource_item = find_item("fixture-change-resource");
    assert_eq!(
        resource_item.version_links,
        vec![TimelineVersionLinkDto::Resource {
            resource_id: "fixture-focus".into(),
            resource_version_id: "fixture-resource-version".into(),
        }]
    );
    let relation_item = find_item("fixture-change-relation");
    assert_eq!(
        relation_item.version_links,
        vec![TimelineVersionLinkDto::Relation {
            relation_id: "fixture-relation".into(),
            relation_version_id: "fixture-relation-version".into(),
        }]
    );
    let binding_item = find_item("fixture-change-binding");
    assert_eq!(
        binding_item.version_links,
        vec![TimelineVersionLinkDto::Relation {
            relation_id: "fixture-binding-relation".into(),
            relation_version_id: "fixture-binding-relation-version".into(),
        }]
    );
    assert!(
        find_item("fixture-change-inference")
            .version_links
            .is_empty()
    );

    let first = service
        .get_timeline(TimelineRequest {
            limit: Some(3),
            cursor: None,
        })
        .unwrap();
    assert_eq!(
        first
            .groups
            .iter()
            .map(|group| group.items.len())
            .sum::<usize>(),
        3
    );
    let cursor = first.page_info.next_cursor().unwrap().to_owned();
    let second = service
        .get_timeline(TimelineRequest {
            limit: Some(3),
            cursor: Some(cursor),
        })
        .unwrap();
    assert_eq!(
        second
            .groups
            .iter()
            .map(|group| group.items.len())
            .sum::<usize>(),
        1
    );
    assert!(second.page_info.next_cursor().is_none());
    let first_ids = first
        .groups
        .iter()
        .flat_map(|group| group.items.iter())
        .map(|item| item.change.change_id.as_str())
        .collect::<BTreeSet<_>>();
    let second_ids = second
        .groups
        .iter()
        .flat_map(|group| group.items.iter())
        .map(|item| item.change.change_id.as_str())
        .collect::<BTreeSet<_>>();
    assert!(first_ids.is_disjoint(&second_ids));
    assert_eq!(first_ids.len() + second_ids.len(), 4);

    let over_limit = service
        .get_timeline(TimelineRequest {
            limit: Some(next_infra_query::service::MAX_TIMELINE_LIMIT + 1),
            cursor: None,
        })
        .unwrap_err();
    assert_eq!(over_limit.code, "invalid_request");
    let zero_limit = service
        .get_timeline(TimelineRequest {
            limit: Some(0),
            cursor: None,
        })
        .unwrap_err();
    assert_eq!(zero_limit.code, "invalid_request");
}
