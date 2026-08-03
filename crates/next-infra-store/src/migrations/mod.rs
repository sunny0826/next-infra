use crate::StoreError;
use rusqlite::{Connection, TransactionBehavior, params};

pub const LATEST_SCHEMA_VERSION: u32 = 2;

const MIGRATION_1: &str = r#"
CREATE TABLE schema_migrations (
    version INTEGER PRIMARY KEY,
    applied_at INTEGER NOT NULL
) STRICT;

CREATE TABLE connections (
    connection_id TEXT PRIMARY KEY,
    connector_type TEXT NOT NULL,
    display_name TEXT NOT NULL,
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    config_json TEXT NOT NULL,
    secret_ref TEXT,
    health TEXT NOT NULL,
    last_success_at INTEGER,
    last_attempt_at INTEGER,
    config_schema_version INTEGER NOT NULL,
    deleted_at INTEGER
) STRICT;

CREATE TABLE connector_state (
    connection_id TEXT PRIMARY KEY REFERENCES connections(connection_id),
    sync_cursor TEXT,
    consecutive_missing_json TEXT NOT NULL DEFAULT '{}'
) STRICT;

CREATE TABLE sync_runs (
    sync_run_id TEXT PRIMARY KEY,
    connection_id TEXT NOT NULL REFERENCES connections(connection_id),
    mode TEXT NOT NULL,
    trigger TEXT NOT NULL,
    started_at INTEGER NOT NULL,
    finished_at INTEGER,
    status TEXT NOT NULL,
    coverage_json TEXT NOT NULL,
    cursor_before TEXT,
    cursor_after TEXT,
    counts_json TEXT NOT NULL,
    errors_json TEXT NOT NULL
) STRICT;

CREATE TABLE resources (
    resource_id TEXT PRIMARY KEY,
    connection_id TEXT NOT NULL REFERENCES connections(connection_id),
    kind TEXT NOT NULL,
    external_id TEXT NOT NULL,
    name TEXT NOT NULL,
    display_name TEXT NOT NULL,
    scope TEXT NOT NULL,
    labels_json TEXT NOT NULL,
    lifecycle TEXT NOT NULL,
    health TEXT NOT NULL,
    attributes_json TEXT NOT NULL,
    attribute_schema_version INTEGER NOT NULL,
    fingerprint TEXT NOT NULL,
    first_seen_at INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL,
    last_changed_at INTEGER NOT NULL,
    last_sync_run_id TEXT NOT NULL REFERENCES sync_runs(sync_run_id),
    UNIQUE(connection_id, kind, external_id)
) STRICT;

CREATE TABLE resource_versions (
    version_id TEXT PRIMARY KEY,
    resource_id TEXT NOT NULL REFERENCES resources(resource_id),
    observed_at INTEGER NOT NULL,
    sync_run_id TEXT NOT NULL REFERENCES sync_runs(sync_run_id),
    normalized_snapshot_json TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    schema_version INTEGER NOT NULL,
    change_summary_json TEXT NOT NULL
) STRICT;

CREATE TABLE relations (
    relation_id TEXT PRIMARY KEY,
    source_resource_id TEXT NOT NULL REFERENCES resources(resource_id),
    target_resource_id TEXT NOT NULL REFERENCES resources(resource_id),
    kind TEXT NOT NULL,
    evidence_type TEXT NOT NULL,
    evidence_key TEXT NOT NULL,
    evidence_json TEXT NOT NULL,
    first_seen_at INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL,
    lifecycle TEXT NOT NULL,
    UNIQUE(source_resource_id, target_resource_id, kind, evidence_type, evidence_key)
) STRICT;

CREATE TABLE relation_versions (
    relation_version_id TEXT PRIMARY KEY,
    relation_id TEXT NOT NULL REFERENCES relations(relation_id),
    observed_at INTEGER NOT NULL,
    normalized_snapshot_json TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    schema_version INTEGER NOT NULL,
    origin_json TEXT NOT NULL
) STRICT;

CREATE TABLE bindings (
    binding_id TEXT PRIMARY KEY,
    source_resource_id TEXT NOT NULL REFERENCES resources(resource_id),
    target_resource_id TEXT NOT NULL REFERENCES resources(resource_id),
    kind TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;

CREATE TABLE changes (
    change_id TEXT PRIMARY KEY,
    subject_type TEXT NOT NULL,
    subject_id TEXT NOT NULL,
    observed_at INTEGER NOT NULL,
    fields_json TEXT NOT NULL,
    origin_json TEXT NOT NULL
) STRICT;

CREATE TABLE maintenance_runs (
    maintenance_run_id TEXT PRIMARY KEY,
    started_at INTEGER NOT NULL,
    finished_at INTEGER,
    status TEXT NOT NULL,
    summary_json TEXT NOT NULL
) STRICT;

CREATE INDEX resources_kind_lifecycle_health_idx ON resources(kind, lifecycle, health);
CREATE INDEX resources_last_seen_idx ON resources(last_seen_at);
CREATE INDEX resource_versions_resource_observed_idx ON resource_versions(resource_id, observed_at DESC);
CREATE INDEX relations_source_lifecycle_idx ON relations(source_resource_id, lifecycle);
CREATE INDEX relations_target_lifecycle_idx ON relations(target_resource_id, lifecycle);
CREATE INDEX changes_observed_idx ON changes(observed_at DESC);
CREATE INDEX sync_runs_connection_started_idx ON sync_runs(connection_id, started_at DESC);
"#;

const MIGRATION_2: &str = r#"
CREATE TABLE projection_metadata (
    singleton_id INTEGER PRIMARY KEY CHECK(singleton_id = 1),
    committed_revision INTEGER NOT NULL,
    committed_at INTEGER NOT NULL
) STRICT;

INSERT INTO projection_metadata(singleton_id, committed_revision, committed_at)
VALUES (1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER));
"#;

pub fn apply(connection: &mut Connection) -> Result<(), StoreError> {
    let current: u32 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(StoreError::Sqlite)?;
    if current > LATEST_SCHEMA_VERSION {
        return Err(StoreError::UnsupportedSchema {
            found: current,
            supported: LATEST_SCHEMA_VERSION,
        });
    }
    if current == LATEST_SCHEMA_VERSION {
        return Ok(());
    }

    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(StoreError::Sqlite)?;
    if current < 1 {
        transaction
            .execute_batch(MIGRATION_1)
            .map_err(StoreError::Sqlite)?;
        transaction
            .execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (?1, unixepoch('subsec') * 1000)",
                params![1_u32],
            )
            .map_err(StoreError::Sqlite)?;
        transaction
            .pragma_update(None, "user_version", 1_u32)
            .map_err(StoreError::Sqlite)?;
    }
    if current < 2 {
        transaction
            .execute_batch(MIGRATION_2)
            .map_err(StoreError::Sqlite)?;
        transaction
            .execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (?1, unixepoch('subsec') * 1000)",
                params![2_u32],
            )
            .map_err(StoreError::Sqlite)?;
        transaction
            .pragma_update(None, "user_version", 2_u32)
            .map_err(StoreError::Sqlite)?;
    }
    transaction.commit().map_err(StoreError::Sqlite)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_one_upgrades_to_projection_metadata_atomically() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(MIGRATION_1).unwrap();
        connection
            .execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (1, 1)",
                [],
            )
            .unwrap();
        connection
            .pragma_update(None, "user_version", 1_u32)
            .unwrap();

        apply(&mut connection).unwrap();

        let version: u32 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        let metadata: (i64, i64) = connection
            .query_row(
                "SELECT committed_revision, committed_at FROM projection_metadata WHERE singleton_id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(version, 2);
        assert_eq!(metadata.0, 0);
        assert!(metadata.1 > 0);
    }
}
