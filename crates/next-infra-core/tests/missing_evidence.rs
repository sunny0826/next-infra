use next_infra_core::{
    Connection, ConnectionId, MissingEvidenceState, Relation, RelationId, Resource, ResourceId,
    Scope, StoreReader, SyncCommit, SyncCoverage, SyncCursor, SyncMode, SyncRun, SyncRunCounts,
    SyncRunId, SyncRunStatus, SyncTrigger, Timestamp,
};
use serde_json::json;
use std::collections::BTreeMap;

fn id<T>(
    value: &str,
    constructor: impl FnOnce(String) -> Result<T, next_infra_core::DomainError>,
) -> T {
    constructor(value.to_owned()).expect("fixture identifier must be valid")
}

#[test]
fn missing_evidence_state_defaults_to_empty_counts_and_round_trips() {
    let scope = id("fixture-scope", Scope::new);
    let empty = MissingEvidenceState::new(scope.clone());
    let resource_id = id("fixture-resource", ResourceId::new);

    assert!(empty.is_empty());
    assert_eq!(empty.count_for(&resource_id), 0);
    assert_eq!(
        serde_json::to_value(&empty).unwrap(),
        json!({
            "scope": "fixture-scope",
            "counts": {},
        })
    );

    let state = MissingEvidenceState::with_counts(
        scope,
        BTreeMap::from([
            (resource_id.clone(), 2),
            (id("fixture-resource-2", ResourceId::new), 1),
        ]),
    );
    let encoded = serde_json::to_string(&state).unwrap();
    let decoded: MissingEvidenceState = serde_json::from_str(&encoded).unwrap();

    assert_eq!(decoded, state);
    assert_eq!(decoded.count_for(&resource_id), 2);
    assert!(!encoded.contains("connection_id"));
}

struct DefaultReader;

impl StoreReader for DefaultReader {
    type Error = std::convert::Infallible;

    fn get_connection(&self, _id: &ConnectionId) -> Result<Option<Connection>, Self::Error> {
        Ok(None)
    }

    fn get_resource(&self, _id: &ResourceId) -> Result<Option<Resource>, Self::Error> {
        Ok(None)
    }

    fn get_relation(&self, _id: &RelationId) -> Result<Option<Relation>, Self::Error> {
        Ok(None)
    }

    fn get_sync_run(&self, _id: &SyncRunId) -> Result<Option<SyncRun>, Self::Error> {
        Ok(None)
    }

    fn sync_cursor(
        &self,
        _connection_id: &ConnectionId,
    ) -> Result<Option<SyncCursor>, Self::Error> {
        Ok(None)
    }

    fn list_resources_for_scope(
        &self,
        _connection_id: &ConnectionId,
        _scope: &Scope,
    ) -> Result<Vec<Resource>, Self::Error> {
        Ok(Vec::new())
    }
}

#[test]
fn store_reader_missing_evidence_defaults_to_absent() {
    let reader = DefaultReader;
    assert_eq!(
        reader
            .missing_evidence_state(
                &id("fixture-connection", ConnectionId::new),
                &id("fixture-scope", Scope::new),
            )
            .unwrap(),
        None
    );
}

#[test]
fn missing_evidence_counts_are_bounded_by_the_u8_contract() {
    let state = MissingEvidenceState::with_counts(
        id("fixture-scope", Scope::new),
        BTreeMap::from([(id("fixture-resource", ResourceId::new), u8::MAX)]),
    );

    assert_eq!(state.counts.values().copied().next(), Some(u8::MAX));
}

#[test]
fn sync_commit_keeps_missing_state_in_the_same_connection_context() {
    let connection_id = id("fixture-connection", ConnectionId::new);
    let scope = id("fixture-scope", Scope::new);
    let sync_run = SyncRun {
        sync_run_id: id("fixture-run", SyncRunId::new),
        connection_id: connection_id.clone(),
        mode: SyncMode::Full,
        trigger: SyncTrigger::User,
        started_at: Timestamp::from_unix_millis(1).unwrap(),
        finished_at: Some(Timestamp::from_unix_millis(2).unwrap()),
        status: SyncRunStatus::Succeeded,
        coverage: SyncCoverage::AuthoritativeFull {
            scope: scope.clone(),
        },
        cursor_before: None,
        cursor_after: None,
        counts: SyncRunCounts::default(),
        errors: Vec::new(),
        warnings: Vec::new(),
    };
    let commit = SyncCommit {
        sync_run,
        resources: Vec::new(),
        resource_versions: Vec::new(),
        relations: Vec::new(),
        relation_versions: Vec::new(),
        changes: Vec::new(),
        cursor_after: None,
        missing_evidence: Some(MissingEvidenceState::new(scope.clone())),
    };

    assert_eq!(commit.sync_run.connection_id, connection_id);
    assert_eq!(commit.missing_evidence.as_ref().unwrap().scope, scope);
}
