//! Goal 2 Connector pipeline integration evidence.

#[cfg(test)]
mod tests {
    use next_infra_connector_api::{
        ConnectionInput, ReadConnector, ResourceLocator, SyncOutcome, SyncRequest,
    };
    use next_infra_connector_catalog::ConnectorCoverageSnapshot;
    use next_infra_connector_contract_tests::{check_descriptor, check_outcome};
    use next_infra_connector_fixture::FixtureConnector;
    use next_infra_core::{
        Connection, ConnectorHealth, ConnectorType, DomainError, ExternalId, Lifecycle,
        ResourceKind, SchemaVersion, Scope, StoreReader, StoreWriter, SyncMode, SyncRunId,
        SyncRunStatus, SyncTrigger, Timestamp,
    };
    use next_infra_normalizer::{AttributeSchema, Normalizer, RelationSchema};
    use next_infra_store::Store;
    use next_infra_sync::{SyncEngine, SyncRunStart};
    use serde_json::json;
    use std::collections::BTreeSet;
    use tempfile::TempDir;

    fn connection() -> Connection {
        Connection {
            connection_id: next_infra_core::ConnectionId::new("fixture-connection").unwrap(),
            connector_type: ConnectorType::new("fixture").unwrap(),
            display_name: "Fixture Connection".into(),
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

    fn normalizer() -> Normalizer {
        Normalizer::new(
            [AttributeSchema {
                kind: ResourceKind::new("fixture.resource").unwrap(),
                schema_version: SchemaVersion::new(1).unwrap(),
                allowed_attributes: BTreeSet::from(["state".to_owned()]),
            }],
            [RelationSchema {
                kind: next_infra_core::RelationKind::new("fixture.depends_on").unwrap(),
                source_kind: ResourceKind::new("fixture.resource").unwrap(),
                target_kind: ResourceKind::new("fixture.resource").unwrap(),
            }],
        )
        .unwrap()
    }

    fn request(
        run_id: &str,
        mode: SyncMode,
        cursor: Option<&str>,
        targeted_resources: Vec<ResourceLocator>,
    ) -> SyncRequest {
        SyncRequest {
            sync_run_id: SyncRunId::new(run_id).unwrap(),
            connection: ConnectionInput {
                connection_id: next_infra_core::ConnectionId::new("fixture-connection").unwrap(),
                connector_type: ConnectorType::new("fixture").unwrap(),
                config: json!({}),
                config_schema_version: SchemaVersion::new(1).unwrap(),
            },
            mode,
            scope: Scope::new("fixture-scope").unwrap(),
            cursor: cursor.map(|value| next_infra_core::SyncCursor::new(value).unwrap()),
            targeted_resources,
        }
    }

    fn start(
        engine: &mut SyncEngine<Store>,
        connection: &Connection,
        request: &SyncRequest,
        trigger: SyncTrigger,
        started_at: i64,
    ) -> next_infra_sync::SyncRunHandle {
        engine
            .start(
                connection,
                SyncRunStart {
                    sync_run_id: request.sync_run_id.clone(),
                    mode: request.mode,
                    trigger,
                    scope: request.scope.clone(),
                    started_at: Timestamp::from_unix_millis(started_at).unwrap(),
                    targeted_resources: request.targeted_resources.clone(),
                },
            )
            .unwrap()
    }

    fn store() -> (TempDir, Store) {
        let tempdir = TempDir::new().unwrap();
        let store = Store::open(&tempdir.path().join("data/next-infra.db")).unwrap();
        (tempdir, store)
    }

    fn commit_complete(
        engine: &mut SyncEngine<Store>,
        connector: &FixtureConnector,
        normalizer: &Normalizer,
        connection: &Connection,
        request: &SyncRequest,
        started_at: i64,
        finished_at: i64,
    ) -> SyncRunStatus {
        let handle = start(
            engine,
            connection,
            request,
            SyncTrigger::Schedule,
            started_at,
        );
        let outcome = connector.replay(request).unwrap();
        assert!(matches!(outcome, SyncOutcome::Complete { .. }));
        assert!(check_outcome(request, &outcome).is_empty());
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
        engine
            .writer()
            .store()
            .get_sync_run(&request.sync_run_id)
            .unwrap()
            .unwrap()
            .status
    }

    fn resource_lifecycle(store: &Store, external_id: &str) -> Lifecycle {
        store
            .list_resources_for_scope(
                &next_infra_core::ConnectionId::new("fixture-connection").unwrap(),
                &Scope::new("fixture-scope").unwrap(),
            )
            .unwrap()
            .into_iter()
            .find(|resource| resource.external_id.as_str() == external_id)
            .unwrap()
            .lifecycle
    }

    #[test]
    fn fixture_full_incremental_and_targeted_replay_commits_through_store() {
        let connector = FixtureConnector::standard().unwrap();
        assert!(check_descriptor(connector.descriptor()).is_empty());
        let normalizer = normalizer();
        let connection = connection();
        let (_tempdir, mut store) = store();
        store.upsert_connection(connection.clone()).unwrap();
        let mut engine = SyncEngine::new(store);

        let full = request("fixture-pipeline-full", SyncMode::Full, None, Vec::new());
        assert_eq!(
            commit_complete(
                &mut engine,
                &connector,
                &normalizer,
                &connection,
                &full,
                1,
                2,
            ),
            SyncRunStatus::Succeeded
        );

        let incremental = request(
            "fixture-pipeline-incremental",
            SyncMode::Incremental,
            Some("cursor-v1"),
            Vec::new(),
        );
        assert_eq!(
            commit_complete(
                &mut engine,
                &connector,
                &normalizer,
                &connection,
                &incremental,
                3,
                4,
            ),
            SyncRunStatus::Succeeded
        );

        let targeted_resource = ResourceLocator {
            kind: ResourceKind::new("fixture.resource").unwrap(),
            external_id: ExternalId::new("fixture-resource-a").unwrap(),
        };
        let targeted = request(
            "fixture-pipeline-targeted",
            SyncMode::Targeted,
            None,
            vec![targeted_resource],
        );
        assert_eq!(
            commit_complete(
                &mut engine,
                &connector,
                &normalizer,
                &connection,
                &targeted,
                5,
                6,
            ),
            SyncRunStatus::Succeeded
        );

        let store = engine.into_store();
        assert_eq!(
            resource_lifecycle(&store, "fixture-resource-a"),
            Lifecycle::Active
        );
        assert_eq!(
            resource_lifecycle(&store, "fixture-resource-b"),
            Lifecycle::Active
        );
    }

    #[test]
    fn partial_then_recovery_does_not_tombstone_omitted_resource() {
        let connector = FixtureConnector::standard().unwrap();
        let normalizer = normalizer();
        let connection = connection();
        let (_tempdir, mut store) = store();
        store.upsert_connection(connection.clone()).unwrap();
        let mut engine = SyncEngine::new(store);
        let full = request("fixture-partial-seed", SyncMode::Full, None, Vec::new());
        commit_complete(
            &mut engine,
            &connector,
            &normalizer,
            &connection,
            &full,
            1,
            2,
        );

        let partial_request = request(
            "fixture-partial",
            SyncMode::Full,
            Some("cursor-partial"),
            Vec::new(),
        );
        let partial_handle = start(
            &mut engine,
            &connection,
            &partial_request,
            SyncTrigger::Schedule,
            3,
        );
        let partial_outcome = connector.replay(&partial_request).unwrap();
        let partial_failure = match &partial_outcome {
            SyncOutcome::Partial { failure, .. } => failure,
            SyncOutcome::Complete { .. } => panic!("fixture partial replay returned complete"),
        };
        assert_eq!(
            partial_failure.code,
            next_infra_core::ErrorCode::RateLimited
        );
        assert!(check_outcome(&partial_request, &partial_outcome).is_empty());
        let partial_batch = normalizer
            .normalize(&partial_request, partial_outcome.batch().clone())
            .unwrap();
        engine
            .commit(
                partial_handle,
                partial_batch,
                Timestamp::from_unix_millis(4).unwrap(),
            )
            .unwrap();
        assert_eq!(
            engine
                .writer()
                .store()
                .get_sync_run(&partial_request.sync_run_id)
                .unwrap()
                .unwrap()
                .status,
            SyncRunStatus::Partial
        );
        assert_eq!(
            resource_lifecycle(engine.writer().store(), "fixture-resource-b"),
            Lifecycle::Active
        );
        assert!(
            engine
                .writer()
                .store()
                .missing_evidence_state(
                    &connection.connection_id,
                    &Scope::new("fixture-scope").unwrap(),
                )
                .unwrap()
                .is_some_and(|state| state.is_empty())
        );

        let recovery_request = request(
            "fixture-recovery",
            SyncMode::Full,
            Some("cursor-recover"),
            Vec::new(),
        );
        assert_eq!(
            commit_complete(
                &mut engine,
                &connector,
                &normalizer,
                &connection,
                &recovery_request,
                5,
                6,
            ),
            SyncRunStatus::Succeeded
        );
        let store = engine.into_store();
        assert_eq!(
            resource_lifecycle(&store, "fixture-resource-b"),
            Lifecycle::Active
        );
    }

    #[test]
    fn consecutive_authoritative_missing_replays_tombstone_only_on_second_miss() {
        let connector = FixtureConnector::standard().unwrap();
        let normalizer = normalizer();
        let connection = connection();
        let (_tempdir, mut store) = store();
        store.upsert_connection(connection.clone()).unwrap();
        let mut engine = SyncEngine::new(store);
        let seed = request("fixture-missing-seed", SyncMode::Full, None, Vec::new());
        commit_complete(
            &mut engine,
            &connector,
            &normalizer,
            &connection,
            &seed,
            1,
            2,
        );

        for (run_id, cursor, started_at, finished_at) in [
            ("fixture-missing-1", "cursor-missing-1", 3, 4),
            ("fixture-missing-2", "cursor-missing-2", 5, 6),
        ] {
            let request = request(run_id, SyncMode::Full, Some(cursor), Vec::new());
            commit_complete(
                &mut engine,
                &connector,
                &normalizer,
                &connection,
                &request,
                started_at,
                finished_at,
            );
            let lifecycle = resource_lifecycle(engine.writer().store(), "fixture-resource-b");
            if cursor == "cursor-missing-1" {
                assert_eq!(lifecycle, Lifecycle::Active);
            } else {
                assert_eq!(lifecycle, Lifecycle::Tombstoned);
            }
        }

        let store = engine.into_store();
        assert_eq!(
            store
                .missing_evidence_state(
                    &connection.connection_id,
                    &Scope::new("fixture-scope").unwrap(),
                )
                .unwrap()
                .unwrap()
                .count_for(
                    &next_infra_core::ResourceId::new(
                        "resource:fixture-connection:fixture.resource:fixture-resource-b",
                    )
                    .unwrap()
                ),
            2
        );
    }

    #[test]
    fn fatal_replay_marks_failed_and_preserves_last_cursor() {
        let connector = FixtureConnector::standard().unwrap();
        let normalizer = normalizer();
        let connection = connection();
        let (_tempdir, mut store) = store();
        store.upsert_connection(connection.clone()).unwrap();
        let mut engine = SyncEngine::new(store);
        let seed = request("fixture-fatal-seed", SyncMode::Full, None, Vec::new());
        commit_complete(
            &mut engine,
            &connector,
            &normalizer,
            &connection,
            &seed,
            1,
            2,
        );

        let fatal_request = request(
            "fixture-fatal",
            SyncMode::Full,
            Some("cursor-fatal"),
            Vec::new(),
        );
        let handle = start(
            &mut engine,
            &connection,
            &fatal_request,
            SyncTrigger::Schedule,
            3,
        );
        let failure = connector.replay(&fatal_request).unwrap_err();
        let error = DomainError {
            code: failure.code,
            message: failure.message,
            retryable: failure.retryable,
        };
        engine
            .fail(handle, error, Timestamp::from_unix_millis(4).unwrap())
            .unwrap();
        let store = engine.into_store();
        let failed = store
            .get_sync_run(&fatal_request.sync_run_id)
            .unwrap()
            .unwrap();
        assert_eq!(failed.status, SyncRunStatus::Failed);
        assert_eq!(
            failed.cursor_after.as_ref().map(|cursor| cursor.as_str()),
            Some("cursor-v1")
        );
        assert_eq!(
            store
                .sync_cursor(&connection.connection_id)
                .unwrap()
                .unwrap()
                .as_str(),
            "cursor-v1"
        );
        assert_eq!(
            resource_lifecycle(&store, "fixture-resource-b"),
            Lifecycle::Active
        );
    }

    #[test]
    fn coverage_catalog_is_descriptor_derived_and_stable() {
        let connector = FixtureConnector::standard().unwrap();
        let descriptor = connector.descriptor();
        assert!(check_descriptor(descriptor).is_empty());
        let first = ConnectorCoverageSnapshot::from_descriptor(descriptor).unwrap();
        let second = ConnectorCoverageSnapshot::from_descriptor(descriptor).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.connector_type.as_str(), "fixture");
        assert_eq!(
            first.modules.len(),
            descriptor.resources.len() + descriptor.relations.len()
        );
        assert!(first.modules.iter().all(|module| {
            module.level == next_infra_core::ConnectorCoverageLevel::Supported
                && module.reason.is_none()
        }));
    }
}
