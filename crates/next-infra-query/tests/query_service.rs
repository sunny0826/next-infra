use next_infra_query::dto::*;
use next_infra_query::service::*;
use serde_json::json;
use std::cell::RefCell;
use std::collections::BTreeSet;

#[derive(Default)]
struct FakeSource {
    search_plan: RefCell<Option<ResourceSearchPlan>>,
    topology_plan: RefCell<Option<TopologyPlan>>,
    fail: bool,
}

impl FakeSource {
    fn snapshot<T>(&self, body: T) -> Result<SourceSnapshot<T>, &'static str> {
        if self.fail {
            Err("sensitive local path must not escape")
        } else {
            Ok(SourceSnapshot {
                metadata: metadata(),
                body,
            })
        }
    }
}

impl QuerySource for FakeSource {
    type Error = &'static str;

    fn search_resources(
        &self,
        plan: &ResourceSearchPlan,
    ) -> Result<SourceSnapshot<SourcePage<ResourceDto>>, Self::Error> {
        self.search_plan.replace(Some(plan.clone()));
        self.snapshot(SourcePage {
            items: vec![resource("fixture-resource-alpha")],
            next_after: Some("fixture-resource-alpha".into()),
        })
    }

    fn get_resource(
        &self,
        resource_id: &str,
        _include: &BTreeSet<ResourceInclude>,
    ) -> Result<SourceSnapshot<Option<ResourceDetailBody>>, Self::Error> {
        self.snapshot(
            (resource_id == "fixture-resource-alpha").then(|| ResourceDetailBody {
                resource: resource(resource_id),
                attributes: json!({"state": "ready"}),
                relations: vec![relation()],
                recent_changes: vec![change()],
                connector_coverage: coverage(),
            }),
        )
    }

    fn get_topology(
        &self,
        plan: &TopologyPlan,
    ) -> Result<SourceSnapshot<Option<TopologyBody>>, Self::Error> {
        self.topology_plan.replace(Some(plan.clone()));
        self.snapshot(Some(TopologyBody {
            nodes: vec![resource(&plan.focus_resource_id)],
            edges: vec![],
            frontier: vec![TopologyFrontierDto {
                resource_id: plan.focus_resource_id.clone(),
                direction: FrontierDirectionDto::Outgoing,
            }],
            truncated: true,
        }))
    }

    fn get_health_summary(&self) -> Result<SourceSnapshot<HealthSummaryBody>, Self::Error> {
        self.snapshot(HealthSummaryBody {
            resource_health: ResourceHealthCountsDto {
                unhealthy: 1,
                ..ResourceHealthCountsDto::default()
            },
            freshness: FreshnessCountsDto {
                expired: 2,
                ..FreshnessCountsDto::default()
            },
            connector_health: ConnectorHealthCountsDto {
                unreachable: 3,
                ..ConnectorHealthCountsDto::default()
            },
        })
    }

    fn get_recent_changes(
        &self,
        _plan: &RecentChangesPlan,
    ) -> Result<SourceSnapshot<SourcePage<ChangeDto>>, Self::Error> {
        self.snapshot(SourcePage {
            items: vec![change()],
            next_after: None,
        })
    }

    fn get_sync_status(
        &self,
        connection_id: &str,
        _recent_run_limit: usize,
    ) -> Result<SourceSnapshot<Option<SyncStatusBody>>, Self::Error> {
        self.snapshot(
            (connection_id == "fixture-connection").then(|| SyncStatusBody {
                connection: connection(),
                recent_runs: vec![],
                next_scheduled_at: None,
            }),
        )
    }

    fn list_connector_coverage(
        &self,
    ) -> Result<SourceSnapshot<Vec<ConnectorCoverageDto>>, Self::Error> {
        self.snapshot(coverage())
    }
}

fn metadata() -> SnapshotMetadata {
    SnapshotMetadata {
        schema_version: QUERY_DTO_SCHEMA_VERSION,
        snapshot_version: "fixture-snapshot-v1".into(),
        generated_at: "2000-01-01T00:00:00Z".into(),
    }
}

fn resource(resource_id: &str) -> ResourceDto {
    ResourceDto {
        resource_id: resource_id.into(),
        connection_id: "fixture-connection".into(),
        kind: "fixture.resource".into(),
        display_name: "Fixture Resource".into(),
        scope: "fixture-scope".into(),
        lifecycle: Lifecycle::Active,
        health: ResourceHealth::Healthy,
        freshness: Freshness::Fresh,
        observed_at: "2000-01-01T00:00:00Z".into(),
    }
}

fn relation() -> RelationDto {
    RelationDto {
        relation_id: "fixture-relation".into(),
        source_resource_id: "fixture-resource-alpha".into(),
        target_resource_id: "fixture-resource-beta".into(),
        kind: "fixture.depends_on".into(),
        lifecycle: Lifecycle::Active,
        evidence_type: EvidenceType::Provider,
        evidence: RelationEvidenceDto::Provider {
            connector_type: "fixture".into(),
            connection_id: "fixture-connection".into(),
            sync_run_id: "fixture-run".into(),
            field_path: "attributes.target".into(),
        },
        last_seen_at: "2000-01-01T00:00:00Z".into(),
    }
}

fn change() -> ChangeDto {
    ChangeDto {
        change_id: "fixture-change".into(),
        subject: ChangeSubjectDto::Resource {
            resource_id: "fixture-resource-alpha".into(),
        },
        observed_at: "2000-01-01T00:00:00Z".into(),
        fields: vec![],
        origin: ChangeOriginDto::SyncRun {
            sync_run_id: "fixture-run".into(),
        },
    }
}

fn connection() -> ConnectionDto {
    ConnectionDto {
        connection_id: "fixture-connection".into(),
        connector_type: "fixture".into(),
        display_name: "Fixture Connection".into(),
        enabled: true,
        health: ConnectorHealth::Healthy,
        last_success_at: Some("2000-01-01T00:00:00Z".into()),
        last_attempt_at: Some("2000-01-01T00:00:00Z".into()),
    }
}

fn coverage() -> Vec<ConnectorCoverageDto> {
    vec![ConnectorCoverageDto {
        connector_type: "fixture".into(),
        connector_version: "1.0.0".into(),
        module: "fixture.resources".into(),
        level: ConnectorCoverageLevelDto::Supported,
        reason: None,
    }]
}

#[test]
fn search_applies_defaults_and_round_trips_an_opaque_cursor() {
    let service = QueryService::new(FakeSource::default());
    let result = service
        .search_resources(SearchResourcesRequest {
            query: Some("  fixture  ".into()),
            cursor: Some("niq1:previous-resource".into()),
            ..SearchResourcesRequest::default()
        })
        .unwrap();

    let plan = service.source().search_plan.borrow().clone().unwrap();
    assert_eq!(plan.query.as_deref(), Some("fixture"));
    assert_eq!(plan.after.as_deref(), Some("previous-resource"));
    assert_eq!(plan.limit, DEFAULT_RESOURCE_LIMIT);
    assert_eq!(
        result.page_info.next_cursor(),
        Some("niq1:fixture-resource-alpha")
    );
}

#[test]
fn invalid_cursor_and_limits_are_rejected_before_source_access() {
    let service = QueryService::new(FakeSource::default());
    let invalid_cursor = service
        .search_resources(SearchResourcesRequest {
            cursor: Some("editable-cursor".into()),
            ..SearchResourcesRequest::default()
        })
        .unwrap_err();
    assert_eq!(invalid_cursor.code, "invalid_request");

    let invalid_topology = service
        .get_topology(GetTopologyRequest {
            focus_resource_id: "fixture-resource-alpha".into(),
            depth: Some(MAX_TOPOLOGY_DEPTH + 1),
            max_nodes: None,
            max_edges: None,
        })
        .unwrap_err();
    assert_eq!(invalid_topology.code, "invalid_request");
}

#[test]
fn topology_defaults_are_bounded_and_frontier_is_preserved() {
    let service = QueryService::new(FakeSource::default());
    let result = service
        .get_topology(GetTopologyRequest {
            focus_resource_id: "fixture-resource-alpha".into(),
            depth: None,
            max_nodes: None,
            max_edges: None,
        })
        .unwrap();
    let plan = service.source().topology_plan.borrow().clone().unwrap();
    assert_eq!(plan.depth, DEFAULT_TOPOLOGY_DEPTH);
    assert_eq!(plan.max_nodes, DEFAULT_TOPOLOGY_NODES);
    assert_eq!(plan.max_edges, DEFAULT_TOPOLOGY_EDGES);
    assert!(result.truncated);
    assert_eq!(result.frontier.len(), 1);
}

#[test]
fn health_dimensions_remain_separate() {
    let result = QueryService::new(FakeSource::default())
        .get_health_summary()
        .unwrap();
    assert_eq!(result.resource_health.unhealthy, 1);
    assert_eq!(result.freshness.expired, 2);
    assert_eq!(result.connector_health.unreachable, 3);
}

#[test]
fn source_errors_are_cleaned_and_not_found_is_distinct() {
    let source_error = QueryService::new(FakeSource {
        fail: true,
        ..FakeSource::default()
    })
    .get_health_summary()
    .unwrap_err();
    assert_eq!(source_error.code, "query_source_unavailable");
    assert!(!source_error.message.contains("path"));

    let missing = QueryService::new(FakeSource::default())
        .get_resource(GetResourceRequest {
            resource_id: "fixture-missing".into(),
            include: BTreeSet::new(),
        })
        .unwrap_err();
    assert_eq!(missing.code, "resource_not_found");
}

#[test]
fn all_query_surfaces_share_snapshot_metadata() {
    let service = QueryService::new(FakeSource::default());
    let detail = service
        .get_resource(GetResourceRequest {
            resource_id: "fixture-resource-alpha".into(),
            include: BTreeSet::from([ResourceInclude::Attributes, ResourceInclude::Relations]),
        })
        .unwrap();
    let changes = service
        .get_recent_changes(RecentChangesRequest::default())
        .unwrap();
    let sync = service
        .get_sync_status(SyncStatusRequest {
            connection_id: "fixture-connection".into(),
            recent_run_limit: None,
        })
        .unwrap();
    let coverage = service.list_connector_coverage().unwrap();

    for snapshot_version in [
        detail.metadata.snapshot_version,
        changes.metadata.snapshot_version,
        sync.metadata.snapshot_version,
        coverage.metadata.snapshot_version,
    ] {
        assert_eq!(snapshot_version, "fixture-snapshot-v1");
    }
}
