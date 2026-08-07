//! Goal 2 Store/Sync integration evidence.

#[cfg(test)]
mod tests {
    use next_infra_core::{
        Change, Connection, ConnectorHealth, ConnectorType, DomainError, EvidenceKey, ExternalId,
        FieldPath, Fingerprint, LabelKey, Lifecycle, MissingEvidenceState, Relation,
        RelationEvidence, RelationId, RelationKind, Resource, ResourceHealth, ResourceId,
        ResourceKind, ResourceVersion, ResourceVersionId, SchemaVersion, Scope, StoreReader,
        StoreWriter, SyncCommit, SyncCoverage, SyncCursor, SyncMode, SyncRun, SyncRunCounts,
        SyncRunId, SyncRunStatus, SyncTrigger, Timestamp,
    };
    use next_infra_normalizer::{ValidatedBatch, ValidatedResource};
    use next_infra_store::Store;
    use next_infra_sync::{SyncEngine, SyncRunStart};
    use serde_json::json;
    use std::collections::BTreeMap;
    use tempfile::TempDir;

    fn id<T>(value: &str, constructor: impl FnOnce(String) -> Result<T, DomainError>) -> T {
        constructor(value.to_owned()).expect("fixture identifier must be valid")
    }

    fn connection() -> Connection {
        Connection {
            connection_id: id("fixture-connection", next_infra_core::ConnectionId::new),
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

    fn scope(value: &str) -> Scope {
        Scope::new(value).unwrap()
    }

    fn store_path(directory: &TempDir) -> std::path::PathBuf {
        directory.path().join("data/next-infra.db")
    }

    fn engine() -> (TempDir, SyncEngine<Store>, Connection) {
        let directory = TempDir::new().unwrap();
        let store = Store::open(&store_path(&directory)).unwrap();
        let connection = connection();
        let mut engine = SyncEngine::new(store);
        engine
            .writer_mut()
            .store_mut()
            .upsert_connection(connection.clone())
            .unwrap();
        (directory, engine, connection)
    }

    fn start(
        engine: &mut SyncEngine<Store>,
        connection: &Connection,
        run_id: &str,
        mode: SyncMode,
        scope: &Scope,
        started_at: i64,
        targeted: bool,
    ) -> next_infra_sync::SyncRunHandle {
        engine
            .start(
                connection,
                SyncRunStart {
                    sync_run_id: id(run_id, SyncRunId::new),
                    mode,
                    trigger: SyncTrigger::User,
                    scope: scope.clone(),
                    started_at: Timestamp::from_unix_millis(started_at).unwrap(),
                    targeted_resources: if targeted {
                        vec![next_infra_connector_api::ResourceLocator {
                            kind: ResourceKind::new("fixture.resource").unwrap(),
                            external_id: ExternalId::new("fixture-resource-a").unwrap(),
                        }]
                    } else {
                        Vec::new()
                    },
                },
            )
            .unwrap()
    }

    fn resource_key(connection: &Connection) -> next_infra_core::ResourceKey {
        next_infra_core::ResourceKey {
            connection_id: connection.connection_id.clone(),
            kind: ResourceKind::new("fixture.resource").unwrap(),
            external_id: ExternalId::new("fixture-resource-a").unwrap(),
        }
    }

    fn batch(
        connection: &Connection,
        run_id: &str,
        scope: &Scope,
        state: Option<&str>,
        coverage: SyncCoverage,
        cursor: Option<&str>,
    ) -> ValidatedBatch {
        let key = resource_key(connection);
        let resources = state.map(|state| {
            vec![ValidatedResource {
                key: key.clone(),
                name: "fixture-a".into(),
                display_name: "Fixture A".into(),
                scope: scope.clone(),
                labels: BTreeMap::<LabelKey, String>::new(),
                health: ResourceHealth::Healthy,
                attributes: json!({"state": state}),
                attribute_schema_version: SchemaVersion::new(1).unwrap(),
                observed_at: Timestamp::from_unix_millis(10).unwrap(),
                fingerprint: Fingerprint::new(format!("fingerprint-{state}")).unwrap(),
            }]
        });
        ValidatedBatch {
            connection_id: connection.connection_id.clone(),
            sync_run_id: id(run_id, SyncRunId::new),
            resources: resources.unwrap_or_default(),
            relations: Vec::new(),
            coverage,
            next_cursor: cursor.map(|value| id(value, SyncCursor::new)),
            warnings: Vec::new(),
            redaction_report: next_infra_connector_api::RedactionReport::default(),
            provider_request_summary: next_infra_connector_api::ProviderRequestSummary::default(),
        }
    }

    fn empty_full_batch(connection: &Connection, run_id: &str, scope: &Scope) -> ValidatedBatch {
        batch(
            connection,
            run_id,
            scope,
            None,
            SyncCoverage::AuthoritativeFull {
                scope: scope.clone(),
            },
            None,
        )
    }

    fn persisted_resource(connection: &Connection, run_id: &str, scope: &Scope) -> Resource {
        Resource {
            resource_id: id(
                "resource:fixture-connection:fixture.resource:fixture-resource-a",
                ResourceId::new,
            ),
            connection_id: connection.connection_id.clone(),
            kind: ResourceKind::new("fixture.resource").unwrap(),
            external_id: ExternalId::new("fixture-resource-a").unwrap(),
            name: "fixture-a".into(),
            display_name: "Fixture A".into(),
            scope: scope.clone(),
            labels: BTreeMap::new(),
            lifecycle: Lifecycle::Active,
            health: ResourceHealth::Healthy,
            attributes: json!({"state": "ready"}),
            attribute_schema_version: SchemaVersion::new(1).unwrap(),
            fingerprint: Fingerprint::new("fingerprint-ready").unwrap(),
            first_seen_at: Timestamp::from_unix_millis(1).unwrap(),
            last_seen_at: Timestamp::from_unix_millis(1).unwrap(),
            last_changed_at: Timestamp::from_unix_millis(1).unwrap(),
            last_sync_run_id: id(run_id, SyncRunId::new),
        }
    }

    fn persisted_version(resource: &Resource, run_id: &str) -> ResourceVersion {
        ResourceVersion {
            version_id: id("fixture-version", ResourceVersionId::new),
            resource_id: resource.resource_id.clone(),
            observed_at: Timestamp::from_unix_millis(1).unwrap(),
            sync_run_id: id(run_id, SyncRunId::new),
            normalized_snapshot: json!({"state": "ready"}),
            fingerprint: resource.fingerprint.clone(),
            schema_version: SchemaVersion::new(1).unwrap(),
            change_summary: Vec::new(),
        }
    }

    fn persisted_run(connection: &Connection, run_id: &str, scope: &Scope) -> SyncRun {
        SyncRun {
            sync_run_id: id(run_id, SyncRunId::new),
            connection_id: connection.connection_id.clone(),
            mode: SyncMode::Full,
            trigger: SyncTrigger::User,
            started_at: Timestamp::from_unix_millis(1).unwrap(),
            finished_at: None,
            status: SyncRunStatus::Running,
            coverage: SyncCoverage::AuthoritativeFull {
                scope: scope.clone(),
            },
            cursor_before: None,
            cursor_after: Some(id("cursor-before", SyncCursor::new)),
            counts: SyncRunCounts::default(),
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn missing_state(scope: &Scope, count: u8) -> MissingEvidenceState {
        MissingEvidenceState::with_counts(
            scope.clone(),
            BTreeMap::from([(
                id(
                    "resource:fixture-connection:fixture.resource:fixture-resource-a",
                    ResourceId::new,
                ),
                count,
            )]),
        )
    }

    #[test]
    fn real_sqlite_commit_is_atomic_for_cursor_projection_and_missing_state() {
        let directory = TempDir::new().unwrap();
        let mut store = Store::open(&store_path(&directory)).unwrap();
        let connection = connection();
        let scope = scope("fixture-scope");
        store.upsert_connection(connection.clone()).unwrap();
        let running = persisted_run(&connection, "fixture-run", &scope);
        store.start_sync_run(running.clone()).unwrap();

        let resource = persisted_resource(&connection, "fixture-run", &scope);
        let invalid_relation = Relation {
            relation_id: id("fixture-relation", RelationId::new),
            source_resource_id: resource.resource_id.clone(),
            target_resource_id: id("missing-target", ResourceId::new),
            kind: RelationKind::new("fixture.depends_on").unwrap(),
            evidence_key: EvidenceKey::new("fixture-evidence").unwrap(),
            evidence: RelationEvidence::Provider {
                connection_id: connection.connection_id.clone(),
                sync_run_id: running.sync_run_id.clone(),
                field_path: FieldPath::new("attributes.target").unwrap(),
            },
            first_seen_at: Timestamp::from_unix_millis(2).unwrap(),
            last_seen_at: Timestamp::from_unix_millis(2).unwrap(),
            lifecycle: Lifecycle::Active,
        };
        let finished = SyncRun {
            status: SyncRunStatus::Succeeded,
            finished_at: Some(Timestamp::from_unix_millis(3).unwrap()),
            cursor_after: Some(id("cursor-after", SyncCursor::new)),
            ..running
        };
        let result = store.commit_sync(SyncCommit {
            sync_run: finished,
            resources: vec![resource.clone()],
            resource_versions: vec![persisted_version(&resource, "fixture-run")],
            relations: vec![invalid_relation],
            relation_versions: Vec::new(),
            changes: Vec::<Change>::new(),
            cursor_after: Some(id("cursor-after", SyncCursor::new)),
            missing_evidence: Some(missing_state(&scope, 1)),
        });

        assert!(result.is_err());
        assert!(store.get_resource(&resource.resource_id).unwrap().is_none());
        assert!(
            store
                .sync_cursor(&connection.connection_id)
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .missing_evidence_state(&connection.connection_id, &scope)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn unchanged_batch_writes_no_new_resource_version() {
        let (_directory, mut engine, connection) = engine();
        let scope = scope("fixture-scope");

        let first = start(
            &mut engine,
            &connection,
            "fixture-run-1",
            SyncMode::Full,
            &scope,
            1,
            false,
        );
        let first_result = engine
            .commit(
                first,
                batch(
                    &connection,
                    "fixture-run-1",
                    &scope,
                    Some("ready"),
                    SyncCoverage::AuthoritativeFull {
                        scope: scope.clone(),
                    },
                    Some("cursor-1"),
                ),
                Timestamp::from_unix_millis(2).unwrap(),
            )
            .unwrap();
        assert_eq!(first_result.resource_versions_written, 1);

        let second = start(
            &mut engine,
            &connection,
            "fixture-run-2",
            SyncMode::Full,
            &scope,
            3,
            false,
        );
        let second_result = engine
            .commit(
                second,
                batch(
                    &connection,
                    "fixture-run-2",
                    &scope,
                    Some("ready"),
                    SyncCoverage::AuthoritativeFull {
                        scope: scope.clone(),
                    },
                    Some("cursor-2"),
                ),
                Timestamp::from_unix_millis(4).unwrap(),
            )
            .unwrap();

        assert_eq!(second_result.resource_versions_written, 0);
        let store = engine.into_store();
        assert_eq!(
            store
                .sync_cursor(&connection.connection_id)
                .unwrap()
                .unwrap()
                .as_str(),
            "cursor-2"
        );
    }

    #[test]
    fn authoritative_absence_tombstones_on_second_pass_and_reappearance_restores_active() {
        let (_directory, mut engine, connection) = engine();
        let scope = scope("fixture-scope");

        let first = start(
            &mut engine,
            &connection,
            "fixture-run-1",
            SyncMode::Full,
            &scope,
            1,
            false,
        );
        engine
            .commit(
                first,
                batch(
                    &connection,
                    "fixture-run-1",
                    &scope,
                    Some("ready"),
                    SyncCoverage::AuthoritativeFull {
                        scope: scope.clone(),
                    },
                    None,
                ),
                Timestamp::from_unix_millis(2).unwrap(),
            )
            .unwrap();

        for (run_id, started_at, finished_at) in [("fixture-run-2", 3, 4), ("fixture-run-3", 5, 6)]
        {
            let handle = start(
                &mut engine,
                &connection,
                run_id,
                SyncMode::Full,
                &scope,
                started_at,
                false,
            );
            engine
                .commit(
                    handle,
                    empty_full_batch(&connection, run_id, &scope),
                    Timestamp::from_unix_millis(finished_at).unwrap(),
                )
                .unwrap();
        }

        let resource_id = id(
            "resource:fixture-connection:fixture.resource:fixture-resource-a",
            ResourceId::new,
        );
        let store = engine.into_store();
        assert_eq!(
            store.get_resource(&resource_id).unwrap().unwrap().lifecycle,
            Lifecycle::Tombstoned
        );
        assert_eq!(
            store
                .missing_evidence_state(&connection.connection_id, &scope)
                .unwrap()
                .unwrap()
                .count_for(&resource_id),
            2
        );

        let mut engine = SyncEngine::new(store);
        let handle = start(
            &mut engine,
            &connection,
            "fixture-run-4",
            SyncMode::Full,
            &scope,
            7,
            false,
        );
        engine
            .commit(
                handle,
                batch(
                    &connection,
                    "fixture-run-4",
                    &scope,
                    Some("ready"),
                    SyncCoverage::AuthoritativeFull {
                        scope: scope.clone(),
                    },
                    None,
                ),
                Timestamp::from_unix_millis(8).unwrap(),
            )
            .unwrap();
        let store = engine.into_store();
        assert_eq!(
            store.get_resource(&resource_id).unwrap().unwrap().lifecycle,
            Lifecycle::Active
        );
        assert_eq!(
            store
                .missing_evidence_state(&connection.connection_id, &scope)
                .unwrap()
                .unwrap()
                .count_for(&resource_id),
            0
        );
    }

    #[test]
    fn partial_incremental_targeted_and_failed_runs_do_not_change_missing_state() {
        let (_directory, mut engine, connection) = engine();
        let scope = scope("fixture-scope");

        let first = start(
            &mut engine,
            &connection,
            "fixture-run-1",
            SyncMode::Full,
            &scope,
            1,
            false,
        );
        engine
            .commit(
                first,
                batch(
                    &connection,
                    "fixture-run-1",
                    &scope,
                    Some("ready"),
                    SyncCoverage::AuthoritativeFull {
                        scope: scope.clone(),
                    },
                    Some("cursor-1"),
                ),
                Timestamp::from_unix_millis(2).unwrap(),
            )
            .unwrap();

        let partial = start(
            &mut engine,
            &connection,
            "fixture-run-2",
            SyncMode::Full,
            &scope,
            3,
            false,
        );
        engine
            .commit(
                partial,
                batch(
                    &connection,
                    "fixture-run-2",
                    &scope,
                    None,
                    SyncCoverage::Partial {
                        scope: Some(scope.clone()),
                        reason: next_infra_core::CoverageGapReason::RateLimited,
                    },
                    Some("cursor-partial"),
                ),
                Timestamp::from_unix_millis(4).unwrap(),
            )
            .unwrap();

        let incremental = start(
            &mut engine,
            &connection,
            "fixture-run-3",
            SyncMode::Incremental,
            &scope,
            5,
            false,
        );
        engine
            .commit(
                incremental,
                batch(
                    &connection,
                    "fixture-run-3",
                    &scope,
                    None,
                    SyncCoverage::Incremental {
                        cursor: id("cursor-partial", SyncCursor::new),
                    },
                    Some("cursor-incremental"),
                ),
                Timestamp::from_unix_millis(6).unwrap(),
            )
            .unwrap();

        let targeted = start(
            &mut engine,
            &connection,
            "fixture-run-4",
            SyncMode::Targeted,
            &scope,
            7,
            true,
        );
        engine
            .commit(
                targeted,
                batch(
                    &connection,
                    "fixture-run-4",
                    &scope,
                    None,
                    SyncCoverage::Targeted {
                        resource_ids: vec![id(
                            "resource:fixture-connection:fixture.resource:fixture-resource-a",
                            ResourceId::new,
                        )],
                    },
                    Some("cursor-targeted"),
                ),
                Timestamp::from_unix_millis(8).unwrap(),
            )
            .unwrap();

        let failed = start(
            &mut engine,
            &connection,
            "fixture-run-5",
            SyncMode::Full,
            &scope,
            9,
            false,
        );
        engine
            .fail(
                failed,
                DomainError {
                    code: next_infra_core::ErrorCode::ProviderUnavailable,
                    message: "fixture provider unavailable".into(),
                    retryable: true,
                },
                Timestamp::from_unix_millis(10).unwrap(),
            )
            .unwrap();

        let store = engine.into_store();
        let state = store
            .missing_evidence_state(&connection.connection_id, &scope)
            .unwrap();
        assert!(state.is_none_or(|state| state.counts.is_empty()));
        assert_eq!(
            store
                .sync_cursor(&connection.connection_id)
                .unwrap()
                .unwrap()
                .as_str(),
            "cursor-targeted"
        );
        assert_eq!(
            store
                .get_resource(&id(
                    "resource:fixture-connection:fixture.resource:fixture-resource-a",
                    ResourceId::new,
                ))
                .unwrap()
                .unwrap()
                .lifecycle,
            Lifecycle::Active
        );
    }

    #[test]
    fn recovery_marks_running_sync_runs_interrupted_in_real_sqlite() {
        let (_directory, mut engine, connection) = engine();
        let scope = scope("fixture-scope");
        let run_id = "fixture-running";
        let handle = start(
            &mut engine,
            &connection,
            run_id,
            SyncMode::Full,
            &scope,
            1,
            false,
        );
        assert_eq!(handle.run.status, SyncRunStatus::Running);

        assert_eq!(
            engine
                .recover(Timestamp::from_unix_millis(9).unwrap())
                .unwrap(),
            1
        );
        let store = engine.into_store();
        let recovered = store
            .get_sync_run(&id(run_id, SyncRunId::new))
            .unwrap()
            .unwrap();
        assert_eq!(recovered.status, SyncRunStatus::Interrupted);
        assert_eq!(
            recovered.finished_at,
            Some(Timestamp::from_unix_millis(9).unwrap())
        );
    }
}
