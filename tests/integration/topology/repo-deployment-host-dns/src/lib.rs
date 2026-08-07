//! Goal 8 synthetic cross-provider topology replay.

#[cfg(test)]
mod tests {
    use next_infra_binding::{BindingInput, BindingService};
    use next_infra_connector_catalog::ConnectorCoverageSnapshot;
    use next_infra_connector_github::github_descriptor;
    use next_infra_core::*;
    use next_infra_query::{
        dto::EvidenceType,
        service::{GetTopologyRequest, QueryService, QuerySource, TopologyPlan},
    };
    use next_infra_runtime::{
        CommittedQuerySource, ConnectorCatalogSnapshot, QueryContextSnapshot, QuerySchedule,
        SharedStore,
    };
    use next_infra_store::Store;
    use serde_json::json;
    use tempfile::TempDir;

    fn timestamp(value: i64) -> Timestamp {
        Timestamp::from_unix_millis(value).unwrap()
    }
    fn id<T>(value: &str, build: impl FnOnce(String) -> Result<T, DomainError>) -> T {
        build(value.into()).unwrap()
    }
    fn connection() -> Connection {
        Connection {
            connection_id: id("fixture-connection", ConnectionId::new),
            connector_type: ConnectorType::new("github").unwrap(),
            display_name: "Fixture".into(),
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
    fn resource(id_value: &str, kind_value: &str) -> Resource {
        Resource {
            resource_id: id(id_value, ResourceId::new),
            connection_id: id("fixture-connection", ConnectionId::new),
            kind: ResourceKind::new(kind_value).unwrap(),
            external_id: id(&format!("external-{id_value}"), ExternalId::new),
            name: id_value.into(),
            display_name: format!("Fixture {id_value}"),
            scope: id("fixture-scope", Scope::new),
            labels: Default::default(),
            lifecycle: Lifecycle::Active,
            health: ResourceHealth::Unknown,
            attributes: json!({}),
            attribute_schema_version: SchemaVersion::new(1).unwrap(),
            fingerprint: id(&format!("fingerprint-{id_value}"), Fingerprint::new),
            first_seen_at: timestamp(1),
            last_seen_at: timestamp(1),
            last_changed_at: timestamp(1),
            last_sync_run_id: id("fixture-run", SyncRunId::new),
        }
    }
    fn relation(
        id_value: &str,
        source: &str,
        target: &str,
        evidence: RelationEvidence,
        lifecycle: Lifecycle,
    ) -> Relation {
        Relation {
            relation_id: id(id_value, RelationId::new),
            source_resource_id: id(source, ResourceId::new),
            target_resource_id: id(target, ResourceId::new),
            kind: RelationKind::new("infra.depends_on").unwrap(),
            evidence_key: id(&format!("evidence-{id_value}"), EvidenceKey::new),
            evidence,
            first_seen_at: timestamp(1),
            last_seen_at: timestamp(1),
            lifecycle,
        }
    }

    #[test]
    fn replay_keeps_provider_configured_and_inferred_evidence_separate_and_bounded() {
        let directory = TempDir::new().unwrap();
        let mut store = Store::open(&directory.path().join("topology.db")).unwrap();
        store.upsert_connection(connection()).unwrap();
        let run = SyncRun {
            sync_run_id: id("fixture-run", SyncRunId::new),
            connection_id: id("fixture-connection", ConnectionId::new),
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
        };
        let mut running = run.clone();
        running.status = SyncRunStatus::Running;
        running.finished_at = None;
        store.start_sync_run(running).unwrap();
        store
            .commit_sync(SyncCommit {
                sync_run: run,
                resources: vec![
                    resource("repo", "github.repository"),
                    resource("deployment", "dokploy.deployment"),
                    resource("host", "ssh.host"),
                    resource("dns", "cloudflare.dns_record"),
                ],
                resource_versions: Vec::new(),
                relations: vec![
                    relation(
                        "provider",
                        "repo",
                        "deployment",
                        RelationEvidence::Provider {
                            connection_id: id("fixture-connection", ConnectionId::new),
                            sync_run_id: id("fixture-run", SyncRunId::new),
                            field_path: FieldPath::new("deployment_id").unwrap(),
                        },
                        Lifecycle::Active,
                    ),
                    relation(
                        "inferred",
                        "host",
                        "dns",
                        RelationEvidence::Inferred {
                            rule_version: id("fixture-rule", RuleVersion::new),
                            input_resource_version_ids: vec![id(
                                "fixture-resource-version",
                                ResourceVersionId::new,
                            )],
                            input_relation_version_ids: vec![id(
                                "fixture-relation-version",
                                RelationVersionId::new,
                            )],
                            confidence: Confidence::from_basis_points(8_500).unwrap(),
                        },
                        Lifecycle::Active,
                    ),
                ],
                relation_versions: Vec::new(),
                changes: Vec::new(),
                cursor_after: None,
                missing_evidence: None,
            })
            .unwrap();
        BindingService::new(&mut store)
            .create(
                BindingInput {
                    source_resource_id: id("deployment", ResourceId::new),
                    target_resource_id: id("host", ResourceId::new),
                    kind: RelationKind::new("infra.depends_on").unwrap(),
                },
                timestamp(3),
            )
            .unwrap();
        let catalog = ConnectorCatalogSnapshot::new([ConnectorCoverageSnapshot::from_descriptor(
            &github_descriptor(),
        )
        .unwrap()]);
        let context = QueryContextSnapshot::new(
            timestamp(3),
            1,
            [(
                id("fixture-connection", ConnectionId::new),
                QuerySchedule::new(1_000, None).unwrap(),
            )],
        )
        .unwrap();
        let source = CommittedQuerySource::new(SharedStore::new(store), catalog, context);
        let raw = source.get_topology(&TopologyPlan {
            focus_resource_id: "repo".into(),
            depth: 3,
            max_nodes: 10,
            max_edges: 10,
        });
        assert!(raw.is_ok(), "{raw:?}");
        let topology = QueryService::new(source)
            .get_topology(GetTopologyRequest {
                focus_resource_id: "repo".into(),
                depth: Some(3),
                max_nodes: Some(10),
                max_edges: Some(10),
            })
            .unwrap();
        assert_eq!(topology.nodes.len(), 4);
        assert_eq!(topology.edges.len(), 3);
        for expected in [
            EvidenceType::Provider,
            EvidenceType::Configured,
            EvidenceType::Inferred,
        ] {
            assert!(
                topology
                    .edges
                    .iter()
                    .any(|edge| edge.evidence_type == expected)
            );
        }
        assert!(
            topology
                .edges
                .iter()
                .any(|edge| edge.evidence_type == EvidenceType::Configured)
        );
        assert!(!topology.truncated);
    }
}
