use crate::{Store, StoreError};
use next_infra_core::*;
use rusqlite::{OptionalExtension, Row, Transaction, params};
use serde::{Serialize, de::DeserializeOwned};
use std::collections::BTreeMap;

impl StoreReader for Store {
    type Error = StoreError;

    fn get_connection(&self, id: &ConnectionId) -> Result<Option<Connection>, Self::Error> {
        self.connection
            .query_row(
                "SELECT connection_id, connector_type, display_name, enabled, config_json, secret_ref, health, last_success_at, last_attempt_at, config_schema_version, deleted_at FROM connections WHERE connection_id = ?1",
                params![id.as_str()],
                read_connection,
            )
            .optional()
            .map_err(StoreError::Sqlite)
    }

    fn get_resource(&self, id: &ResourceId) -> Result<Option<Resource>, Self::Error> {
        self.connection
            .query_row(
                "SELECT resource_id, connection_id, kind, external_id, name, display_name, scope, labels_json, lifecycle, health, attributes_json, attribute_schema_version, fingerprint, first_seen_at, last_seen_at, last_changed_at, last_sync_run_id FROM resources WHERE resource_id = ?1",
                params![id.as_str()],
                read_resource,
            )
            .optional()
            .map_err(StoreError::Sqlite)
    }

    fn get_relation(&self, id: &RelationId) -> Result<Option<Relation>, Self::Error> {
        self.connection
            .query_row(
                "SELECT relation_id, source_resource_id, target_resource_id, kind, evidence_key, evidence_json, first_seen_at, last_seen_at, lifecycle FROM relations WHERE relation_id = ?1",
                params![id.as_str()],
                read_relation,
            )
            .optional()
            .map_err(StoreError::Sqlite)
    }

    fn latest_relation_version_fingerprint(
        &self,
        id: &RelationId,
    ) -> Result<Option<Fingerprint>, Self::Error> {
        let fingerprint = self
            .connection
            .query_row(
                "SELECT fingerprint FROM relation_versions WHERE relation_id = ?1 ORDER BY observed_at DESC, relation_version_id DESC LIMIT 1",
                params![id.as_str()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(StoreError::Sqlite)?;
        fingerprint
            .map(Fingerprint::new)
            .transpose()
            .map_err(domain_error)
    }

    fn get_sync_run(&self, id: &SyncRunId) -> Result<Option<SyncRun>, Self::Error> {
        self.connection
            .query_row(
                "SELECT sync_run_id, connection_id, mode, trigger, started_at, finished_at, status, coverage_json, cursor_before, cursor_after, counts_json, errors_json FROM sync_runs WHERE sync_run_id = ?1",
                params![id.as_str()],
                read_sync_run,
            )
            .optional()
            .map_err(StoreError::Sqlite)
    }

    fn sync_cursor(&self, connection_id: &ConnectionId) -> Result<Option<SyncCursor>, Self::Error> {
        let cursor = self
            .connection
            .query_row(
                "SELECT sync_cursor FROM connector_state WHERE connection_id = ?1",
                params![connection_id.as_str()],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(StoreError::Sqlite)?
            .flatten();
        cursor
            .map(SyncCursor::new)
            .transpose()
            .map_err(domain_error)
    }

    fn missing_evidence_state(
        &self,
        connection_id: &ConnectionId,
        scope: &Scope,
    ) -> Result<Option<MissingEvidenceState>, Self::Error> {
        let persisted = self
            .connection
            .query_row(
                "SELECT consecutive_missing_json FROM connector_state WHERE connection_id = ?1",
                params![connection_id.as_str()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(StoreError::Sqlite)?;
        persisted
            .map(|value| read_missing_evidence_state(&value, scope))
            .transpose()
            .map(|state| state.flatten())
    }

    fn list_resources_for_scope(
        &self,
        connection_id: &ConnectionId,
        scope: &Scope,
    ) -> Result<Vec<Resource>, Self::Error> {
        let mut statement = self
            .connection
            .prepare("SELECT resource_id, connection_id, kind, external_id, name, display_name, scope, labels_json, lifecycle, health, attributes_json, attribute_schema_version, fingerprint, first_seen_at, last_seen_at, last_changed_at, last_sync_run_id FROM resources WHERE connection_id = ?1 AND scope = ?2 ORDER BY resource_id")
            .map_err(StoreError::Sqlite)?;
        let rows = statement
            .query_map(
                params![connection_id.as_str(), scope.as_str()],
                read_resource,
            )
            .map_err(StoreError::Sqlite)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::Sqlite)
    }
}

impl StoreWriter for Store {
    type Error = StoreError;

    fn upsert_connection(&mut self, connection: Connection) -> Result<(), Self::Error> {
        let config = serde_json::to_string(&connection.config).map_err(StoreError::Json)?;
        let secret_ref = connection.secret_ref.as_ref().map(json_text).transpose()?;
        self.connection
            .execute(
                "INSERT INTO connections(connection_id, connector_type, display_name, enabled, config_json, secret_ref, health, last_success_at, last_attempt_at, config_schema_version, deleted_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT(connection_id) DO UPDATE SET connector_type=excluded.connector_type, display_name=excluded.display_name, enabled=excluded.enabled, config_json=excluded.config_json, secret_ref=excluded.secret_ref, health=excluded.health, last_success_at=excluded.last_success_at, last_attempt_at=excluded.last_attempt_at, config_schema_version=excluded.config_schema_version, deleted_at=excluded.deleted_at",
                params![
                    connection.connection_id.as_str(),
                    connection.connector_type.as_str(),
                    connection.display_name,
                    connection.enabled,
                    config,
                    secret_ref,
                    enum_text(&connection.health)?,
                    timestamp_option(connection.last_success_at),
                    timestamp_option(connection.last_attempt_at),
                    connection.config_schema_version.get(),
                    timestamp_option(connection.deleted_at),
                ],
            )
            .map_err(StoreError::Sqlite)?;
        Ok(())
    }

    fn start_sync_run(&mut self, sync_run: SyncRun) -> Result<(), Self::Error> {
        if sync_run.status != SyncRunStatus::Running || sync_run.finished_at.is_some() {
            return Err(StoreError::Contract(
                "start_sync_run requires running status without finished_at".into(),
            ));
        }
        insert_sync_run(&self.connection, &sync_run)
    }

    fn commit_sync(&mut self, commit: SyncCommit) -> Result<CommitResult, Self::Error> {
        if commit.sync_run.status == SyncRunStatus::Running || commit.sync_run.finished_at.is_none()
        {
            return Err(StoreError::Contract(
                "commit_sync requires a finished sync run".into(),
            ));
        }
        validate_commit(&commit)?;
        let transaction = self.connection.transaction().map_err(StoreError::Sqlite)?;
        complete_sync_run(&transaction, &commit.sync_run)?;

        let mut result = CommitResult::default();
        for resource in &commit.resources {
            upsert_resource(&transaction, resource)?;
            result.resources_written += 1;
        }
        for version in &commit.resource_versions {
            result.resource_versions_written += insert_resource_version(&transaction, version)?;
        }
        for relation in &commit.relations {
            upsert_relation(&transaction, relation)?;
            result.relations_written += 1;
        }
        for version in &commit.relation_versions {
            result.relation_versions_written += insert_relation_version(&transaction, version)?;
        }
        for change in &commit.changes {
            result.changes_written += insert_change(&transaction, change)?;
        }

        let missing_evidence_json = missing_evidence_json(&transaction, &commit)?;
        transaction
            .execute(
                "INSERT INTO connector_state(connection_id, sync_cursor, consecutive_missing_json) VALUES (?1, ?2, ?3)
                 ON CONFLICT(connection_id) DO UPDATE SET sync_cursor=excluded.sync_cursor, consecutive_missing_json=excluded.consecutive_missing_json",
                params![
                    commit.sync_run.connection_id.as_str(),
                    commit.cursor_after.as_ref().map(SyncCursor::as_str),
                    missing_evidence_json,
                ],
            )
            .map_err(StoreError::Sqlite)?;
        transaction.commit().map_err(StoreError::Sqlite)?;
        Ok(result)
    }

    fn mark_running_syncs_interrupted(&mut self, at: Timestamp) -> Result<usize, Self::Error> {
        self.connection
            .execute(
                "UPDATE sync_runs SET status = 'interrupted', finished_at = ?1 WHERE status = 'running'",
                params![at.unix_millis()],
            )
            .map_err(StoreError::Sqlite)
    }
}

fn insert_sync_run(connection: &rusqlite::Connection, run: &SyncRun) -> Result<(), StoreError> {
    connection
        .execute(
            "INSERT INTO sync_runs(sync_run_id, connection_id, mode, trigger, started_at, finished_at, status, coverage_json, cursor_before, cursor_after, counts_json, errors_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                run.sync_run_id.as_str(), run.connection_id.as_str(), enum_text(&run.mode)?, enum_text(&run.trigger)?, run.started_at.unix_millis(), timestamp_option(run.finished_at), enum_text(&run.status)?, json_text(&run.coverage)?, run.cursor_before.as_ref().map(SyncCursor::as_str), run.cursor_after.as_ref().map(SyncCursor::as_str), json_text(&run.counts)?, json_text(&run.errors)?,
            ],
        )
        .map_err(StoreError::Sqlite)?;
    Ok(())
}

fn complete_sync_run(transaction: &Transaction<'_>, run: &SyncRun) -> Result<(), StoreError> {
    let updated = transaction
        .execute(
            "UPDATE sync_runs SET finished_at=?2, status=?3, coverage_json=?4, cursor_after=?5, counts_json=?6, errors_json=?7 WHERE sync_run_id=?1 AND connection_id=?8",
            params![run.sync_run_id.as_str(), timestamp_option(run.finished_at), enum_text(&run.status)?, json_text(&run.coverage)?, run.cursor_after.as_ref().map(SyncCursor::as_str), json_text(&run.counts)?, json_text(&run.errors)?, run.connection_id.as_str()],
        )
        .map_err(StoreError::Sqlite)?;
    if updated != 1 {
        return Err(StoreError::Contract(
            "sync run must be started before completion".into(),
        ));
    }
    Ok(())
}

fn upsert_resource(transaction: &Transaction<'_>, resource: &Resource) -> Result<(), StoreError> {
    if let Some((connection_id, kind, external_id)) = transaction
        .query_row(
            "SELECT connection_id, kind, external_id FROM resources WHERE resource_id = ?1",
            params![resource.resource_id.as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()
        .map_err(StoreError::Sqlite)?
        && (connection_id != resource.connection_id.as_str()
            || kind != resource.kind.as_str()
            || external_id != resource.external_id.as_str())
    {
        return Err(StoreError::Contract(
            "resource stable identity cannot change".into(),
        ));
    }
    transaction.execute(
        "INSERT INTO resources(resource_id, connection_id, kind, external_id, name, display_name, scope, labels_json, lifecycle, health, attributes_json, attribute_schema_version, fingerprint, first_seen_at, last_seen_at, last_changed_at, last_sync_run_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
         ON CONFLICT(resource_id) DO UPDATE SET name=excluded.name, display_name=excluded.display_name, scope=excluded.scope, labels_json=excluded.labels_json, lifecycle=excluded.lifecycle, health=excluded.health, attributes_json=excluded.attributes_json, attribute_schema_version=excluded.attribute_schema_version, fingerprint=excluded.fingerprint, last_seen_at=excluded.last_seen_at, last_changed_at=excluded.last_changed_at, last_sync_run_id=excluded.last_sync_run_id",
        params![resource.resource_id.as_str(), resource.connection_id.as_str(), resource.kind.as_str(), resource.external_id.as_str(), resource.name, resource.display_name, resource.scope.as_str(), json_text(&resource.labels)?, enum_text(&resource.lifecycle)?, enum_text(&resource.health)?, json_text(&resource.attributes)?, resource.attribute_schema_version.get(), resource.fingerprint.as_str(), resource.first_seen_at.unix_millis(), resource.last_seen_at.unix_millis(), resource.last_changed_at.unix_millis(), resource.last_sync_run_id.as_str()],
    ).map_err(StoreError::Sqlite)?;
    Ok(())
}

fn insert_resource_version(
    transaction: &Transaction<'_>,
    version: &ResourceVersion,
) -> Result<usize, StoreError> {
    transaction.execute(
        "INSERT OR IGNORE INTO resource_versions(version_id, resource_id, observed_at, sync_run_id, normalized_snapshot_json, fingerprint, schema_version, change_summary_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![version.version_id.as_str(), version.resource_id.as_str(), version.observed_at.unix_millis(), version.sync_run_id.as_str(), json_text(&version.normalized_snapshot)?, version.fingerprint.as_str(), version.schema_version.get(), json_text(&version.change_summary)?],
    ).map_err(StoreError::Sqlite)
}

fn upsert_relation(transaction: &Transaction<'_>, relation: &Relation) -> Result<(), StoreError> {
    if let Some((source, target, kind, evidence_type, evidence_key)) = transaction
        .query_row(
            "SELECT source_resource_id, target_resource_id, kind, evidence_type, evidence_key FROM relations WHERE relation_id = ?1",
            params![relation.relation_id.as_str()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, String>(4)?)),
        )
        .optional()
        .map_err(StoreError::Sqlite)?
    {
        let incoming_type = enum_text(&relation.evidence.evidence_type())?;
        if source != relation.source_resource_id.as_str()
            || target != relation.target_resource_id.as_str()
            || kind != relation.kind.as_str()
            || evidence_type != incoming_type
            || evidence_key != relation.evidence_key.as_str()
        {
            return Err(StoreError::Contract(
                "relation stable identity cannot change".into(),
            ));
        }
    }
    transaction.execute(
        "INSERT INTO relations(relation_id, source_resource_id, target_resource_id, kind, evidence_type, evidence_key, evidence_json, first_seen_at, last_seen_at, lifecycle) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(relation_id) DO UPDATE SET evidence_json=excluded.evidence_json, last_seen_at=excluded.last_seen_at, lifecycle=excluded.lifecycle",
        params![relation.relation_id.as_str(), relation.source_resource_id.as_str(), relation.target_resource_id.as_str(), relation.kind.as_str(), enum_text(&relation.evidence.evidence_type())?, relation.evidence_key.as_str(), json_text(&relation.evidence)?, relation.first_seen_at.unix_millis(), relation.last_seen_at.unix_millis(), enum_text(&relation.lifecycle)?],
    ).map_err(StoreError::Sqlite)?;
    Ok(())
}

fn insert_relation_version(
    transaction: &Transaction<'_>,
    version: &RelationVersion,
) -> Result<usize, StoreError> {
    transaction.execute(
        "INSERT OR IGNORE INTO relation_versions(relation_version_id, relation_id, observed_at, normalized_snapshot_json, fingerprint, schema_version, origin_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![version.relation_version_id.as_str(), version.relation_id.as_str(), version.observed_at.unix_millis(), json_text(&version.normalized_snapshot)?, version.fingerprint.as_str(), version.schema_version.get(), json_text(&version.origin)?],
    ).map_err(StoreError::Sqlite)
}

fn insert_change(transaction: &Transaction<'_>, change: &Change) -> Result<usize, StoreError> {
    let (subject_type, subject_id) = match &change.subject {
        ChangeSubject::Resource { resource_id } => ("resource", resource_id.as_str()),
        ChangeSubject::Relation { relation_id } => ("relation", relation_id.as_str()),
    };
    transaction.execute(
        "INSERT OR IGNORE INTO changes(change_id, subject_type, subject_id, observed_at, fields_json, origin_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![change.change_id.as_str(), subject_type, subject_id, change.observed_at.unix_millis(), json_text(&change.fields)?, json_text(&change.origin)?],
    ).map_err(StoreError::Sqlite)
}

fn read_connection(row: &Row<'_>) -> rusqlite::Result<Connection> {
    Ok(Connection {
        connection_id: wrapped(row.get(0)?)?,
        connector_type: wrapped(row.get(1)?)?,
        display_name: row.get(2)?,
        enabled: row.get(3)?,
        config: json_value(row.get(4)?)?,
        secret_ref: optional_json(row.get(5)?)?,
        health: enum_value(row.get(6)?)?,
        last_success_at: optional_timestamp(row.get(7)?)?,
        last_attempt_at: optional_timestamp(row.get(8)?)?,
        config_schema_version: schema_version(row.get(9)?)?,
        deleted_at: optional_timestamp(row.get(10)?)?,
    })
}

fn read_resource(row: &Row<'_>) -> rusqlite::Result<Resource> {
    Ok(Resource {
        resource_id: wrapped(row.get(0)?)?,
        connection_id: wrapped(row.get(1)?)?,
        kind: wrapped(row.get(2)?)?,
        external_id: wrapped(row.get(3)?)?,
        name: row.get(4)?,
        display_name: row.get(5)?,
        scope: wrapped(row.get(6)?)?,
        labels: json(row.get(7)?)?,
        lifecycle: enum_value(row.get(8)?)?,
        health: enum_value(row.get(9)?)?,
        attributes: json_value(row.get(10)?)?,
        attribute_schema_version: schema_version(row.get(11)?)?,
        fingerprint: wrapped(row.get(12)?)?,
        first_seen_at: timestamp(row.get(13)?)?,
        last_seen_at: timestamp(row.get(14)?)?,
        last_changed_at: timestamp(row.get(15)?)?,
        last_sync_run_id: wrapped(row.get(16)?)?,
    })
}

fn read_relation(row: &Row<'_>) -> rusqlite::Result<Relation> {
    Ok(Relation {
        relation_id: wrapped(row.get(0)?)?,
        source_resource_id: wrapped(row.get(1)?)?,
        target_resource_id: wrapped(row.get(2)?)?,
        kind: wrapped(row.get(3)?)?,
        evidence_key: wrapped(row.get(4)?)?,
        evidence: json(row.get(5)?)?,
        first_seen_at: timestamp(row.get(6)?)?,
        last_seen_at: timestamp(row.get(7)?)?,
        lifecycle: enum_value(row.get(8)?)?,
    })
}

fn read_sync_run(row: &Row<'_>) -> rusqlite::Result<SyncRun> {
    Ok(SyncRun {
        sync_run_id: wrapped(row.get(0)?)?,
        connection_id: wrapped(row.get(1)?)?,
        mode: enum_value(row.get(2)?)?,
        trigger: enum_value(row.get(3)?)?,
        started_at: timestamp(row.get(4)?)?,
        finished_at: optional_timestamp(row.get(5)?)?,
        status: enum_value(row.get(6)?)?,
        coverage: json(row.get(7)?)?,
        cursor_before: optional_wrapped(row.get(8)?)?,
        cursor_after: optional_wrapped(row.get(9)?)?,
        counts: json(row.get(10)?)?,
        errors: json(row.get(11)?)?,
    })
}

fn json_text(value: &impl Serialize) -> Result<String, StoreError> {
    serde_json::to_string(value).map_err(StoreError::Json)
}

type PersistedMissingEvidence = BTreeMap<String, BTreeMap<String, u8>>;

fn read_missing_evidence_state(
    value: &str,
    scope: &Scope,
) -> Result<Option<MissingEvidenceState>, StoreError> {
    let persisted: PersistedMissingEvidence =
        serde_json::from_str(value).map_err(StoreError::Json)?;
    let Some(counts) = persisted.get(scope.as_str()) else {
        return Ok(None);
    };
    let counts = counts
        .iter()
        .map(|(resource_id, count)| {
            ResourceId::new(resource_id.clone())
                .map(|resource_id| (resource_id, *count))
                .map_err(domain_error)
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    Ok(Some(MissingEvidenceState::with_counts(
        scope.clone(),
        counts,
    )))
}

fn missing_evidence_json(
    transaction: &Transaction<'_>,
    commit: &SyncCommit,
) -> Result<String, StoreError> {
    let existing = transaction
        .query_row(
            "SELECT consecutive_missing_json FROM connector_state WHERE connection_id = ?1",
            params![commit.sync_run.connection_id.as_str()],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(StoreError::Sqlite)?;
    let mut persisted: PersistedMissingEvidence = existing
        .map(|value| serde_json::from_str(&value).map_err(StoreError::Json))
        .transpose()?
        .unwrap_or_default();
    if let Some(state) = &commit.missing_evidence {
        persisted.insert(
            state.scope.as_str().to_owned(),
            state
                .counts
                .iter()
                .map(|(resource_id, count)| (resource_id.as_str().to_owned(), *count))
                .collect(),
        );
    }
    serde_json::to_string(&persisted).map_err(StoreError::Json)
}

fn enum_text(value: &impl Serialize) -> Result<String, StoreError> {
    let value = serde_json::to_value(value).map_err(StoreError::Json)?;
    value
        .as_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| StoreError::Contract("enum did not serialize as a string".into()))
}

fn timestamp_option(value: Option<Timestamp>) -> Option<i64> {
    value.map(Timestamp::unix_millis)
}

fn json<T: DeserializeOwned>(value: String) -> rusqlite::Result<T> {
    serde_json::from_str(&value).map_err(conversion_error)
}

fn json_value(value: String) -> rusqlite::Result<serde_json::Value> {
    json(value)
}

fn wrapped<T: DeserializeOwned>(value: String) -> rusqlite::Result<T> {
    serde_json::from_value(serde_json::Value::String(value)).map_err(conversion_error)
}

fn optional_wrapped<T: DeserializeOwned>(value: Option<String>) -> rusqlite::Result<Option<T>> {
    value.map(wrapped).transpose()
}

fn optional_json<T: DeserializeOwned>(value: Option<String>) -> rusqlite::Result<Option<T>> {
    value.map(json).transpose()
}

fn enum_value<T: DeserializeOwned>(value: String) -> rusqlite::Result<T> {
    wrapped(value)
}

fn timestamp(value: i64) -> rusqlite::Result<Timestamp> {
    Timestamp::from_unix_millis(value).map_err(conversion_error)
}

fn optional_timestamp(value: Option<i64>) -> rusqlite::Result<Option<Timestamp>> {
    value.map(timestamp).transpose()
}

fn schema_version(value: u32) -> rusqlite::Result<SchemaVersion> {
    SchemaVersion::new(value).map_err(conversion_error)
}

fn conversion_error(error: impl std::error::Error + Send + Sync + 'static) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}

fn domain_error(error: DomainError) -> StoreError {
    StoreError::Contract(error.to_string())
}

fn validate_commit(commit: &SyncCommit) -> Result<(), StoreError> {
    let run = &commit.sync_run;
    if commit.cursor_after != run.cursor_after {
        return Err(StoreError::Contract(
            "commit cursor must match SyncRun cursor_after".into(),
        ));
    }
    if commit.resources.iter().any(|resource| {
        resource.connection_id != run.connection_id || resource.last_sync_run_id != run.sync_run_id
    }) {
        return Err(StoreError::Contract(
            "resource provenance does not match SyncRun".into(),
        ));
    }
    if commit
        .resource_versions
        .iter()
        .any(|version| version.sync_run_id != run.sync_run_id)
    {
        return Err(StoreError::Contract(
            "resource version provenance does not match SyncRun".into(),
        ));
    }
    if commit
        .relations
        .iter()
        .any(|relation| match &relation.evidence {
            RelationEvidence::Provider {
                connection_id,
                sync_run_id,
                ..
            } => connection_id != &run.connection_id || sync_run_id != &run.sync_run_id,
            RelationEvidence::Configured { .. } | RelationEvidence::Inferred { .. } => false,
        })
    {
        return Err(StoreError::Contract(
            "provider relation provenance does not match SyncRun".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;
    use tempfile::TempDir;

    fn id<T>(value: &str, constructor: impl FnOnce(String) -> Result<T, DomainError>) -> T {
        constructor(value.to_owned()).unwrap()
    }

    fn timestamp_at(value: i64) -> Timestamp {
        Timestamp::from_unix_millis(value).unwrap()
    }

    fn connection() -> Connection {
        Connection {
            connection_id: id("fixture-connection", ConnectionId::new),
            connector_type: ConnectorType::new("fixture").unwrap(),
            display_name: "Fixture".into(),
            enabled: true,
            config: json!({}),
            secret_ref: None,
            health: ConnectorHealth::Healthy,
            last_success_at: None,
            last_attempt_at: Some(timestamp_at(1)),
            config_schema_version: SchemaVersion::new(1).unwrap(),
            deleted_at: None,
        }
    }

    fn secret_ref() -> SecretRef {
        SecretRef::new(SecretRefInput {
            backend: SecretBackend::MacosDataProtectionKeychainV1,
            service: "dev.example.next-infra.provider-secret.v1".into(),
            account: "connection/fixture-connection/kind/api-token/generation/fixture-generation"
                .into(),
            secret_kind: SecretKind::ApiToken,
            generation_id: "fixture-generation".into(),
            created_at: timestamp_at(1),
            last_verified_at: timestamp_at(2),
            permission_scope_summary: "fixture read-only scope".into(),
        })
        .unwrap()
    }

    fn run(status: SyncRunStatus, finished_at: Option<Timestamp>) -> SyncRun {
        SyncRun {
            sync_run_id: id("fixture-run", SyncRunId::new),
            connection_id: id("fixture-connection", ConnectionId::new),
            mode: SyncMode::Full,
            trigger: SyncTrigger::User,
            started_at: timestamp_at(1),
            finished_at,
            status,
            coverage: SyncCoverage::AuthoritativeFull {
                scope: id("fixture-scope", Scope::new),
            },
            cursor_before: Some(id("cursor-before", SyncCursor::new)),
            cursor_after: Some(id("cursor-after", SyncCursor::new)),
            counts: SyncRunCounts::default(),
            errors: Vec::new(),
        }
    }

    fn resource() -> Resource {
        Resource {
            resource_id: id("fixture-resource", ResourceId::new),
            connection_id: id("fixture-connection", ConnectionId::new),
            kind: ResourceKind::new("fixture.resource").unwrap(),
            external_id: id("external-1", ExternalId::new),
            name: "fixture".into(),
            display_name: "Fixture Resource".into(),
            scope: id("fixture-scope", Scope::new),
            labels: BTreeMap::new(),
            lifecycle: Lifecycle::Active,
            health: ResourceHealth::Healthy,
            attributes: json!({"state": "ready"}),
            attribute_schema_version: SchemaVersion::new(1).unwrap(),
            fingerprint: id("fingerprint-1", Fingerprint::new),
            first_seen_at: timestamp_at(1),
            last_seen_at: timestamp_at(2),
            last_changed_at: timestamp_at(2),
            last_sync_run_id: id("fixture-run", SyncRunId::new),
        }
    }

    fn version() -> ResourceVersion {
        ResourceVersion {
            version_id: id("fixture-version", ResourceVersionId::new),
            resource_id: id("fixture-resource", ResourceId::new),
            observed_at: timestamp_at(2),
            sync_run_id: id("fixture-run", SyncRunId::new),
            normalized_snapshot: json!({"state": "ready"}),
            fingerprint: id("fingerprint-1", Fingerprint::new),
            schema_version: SchemaVersion::new(1).unwrap(),
            change_summary: Vec::new(),
        }
    }

    fn relation(sync_run_id: &str) -> Relation {
        Relation {
            relation_id: id("fixture-relation", RelationId::new),
            source_resource_id: id("fixture-resource", ResourceId::new),
            target_resource_id: id("fixture-resource", ResourceId::new),
            kind: RelationKind::new("fixture.depends_on").unwrap(),
            evidence_key: id("fixture-evidence", EvidenceKey::new),
            evidence: RelationEvidence::Provider {
                connection_id: id("fixture-connection", ConnectionId::new),
                sync_run_id: id(sync_run_id, SyncRunId::new),
                field_path: id("attributes.target", FieldPath::new),
            },
            first_seen_at: timestamp_at(2),
            last_seen_at: timestamp_at(2),
            lifecycle: Lifecycle::Active,
        }
    }

    fn relation_version(
        version_id: &str,
        fingerprint: &str,
        sync_run_id: &str,
        observed_at: i64,
    ) -> RelationVersion {
        RelationVersion {
            relation_version_id: id(version_id, RelationVersionId::new),
            relation_id: id("fixture-relation", RelationId::new),
            observed_at: timestamp_at(observed_at),
            normalized_snapshot: json!({"relation": "fixture"}),
            fingerprint: id(fingerprint, Fingerprint::new),
            schema_version: SchemaVersion::new(1).unwrap(),
            origin: OriginRef::SyncRun {
                sync_run_id: id(sync_run_id, SyncRunId::new),
            },
        }
    }

    fn store() -> (TempDir, Store) {
        let directory = TempDir::new().unwrap();
        let store = Store::open(&directory.path().join("data/next-infra.db")).unwrap();
        (directory, store)
    }

    fn missing_state(scope: &str, resource_id: &str, count: u8) -> MissingEvidenceState {
        MissingEvidenceState::with_counts(
            id(scope, Scope::new),
            BTreeMap::from([(id(resource_id, ResourceId::new), count)]),
        )
    }

    #[test]
    fn connection_round_trips_structured_secret_reference_without_secret_value() {
        let (_directory, mut store) = store();
        let mut connection = connection();
        connection.secret_ref = Some(secret_ref());
        store.upsert_connection(connection.clone()).unwrap();

        let persisted = store
            .get_connection(&connection.connection_id)
            .unwrap()
            .unwrap();
        assert_eq!(persisted.secret_ref, connection.secret_ref);
        let raw: String = store
            .connection
            .query_row(
                "SELECT secret_ref FROM connections WHERE connection_id = ?1",
                params![connection.connection_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert!(raw.contains("macos_data_protection_keychain_v1"));
        assert!(!raw.contains("secret_value"));
    }

    #[test]
    fn missing_evidence_is_absent_before_first_commit() {
        let (_directory, mut store) = store();
        store.upsert_connection(connection()).unwrap();

        assert_eq!(
            store
                .missing_evidence_state(
                    &id("fixture-connection", ConnectionId::new),
                    &id("fixture-scope", Scope::new),
                )
                .unwrap(),
            None
        );
    }

    #[test]
    fn missing_evidence_round_trips_and_isolated_by_scope() {
        let (_directory, mut store) = store();
        let connection_id = id("fixture-connection", ConnectionId::new);
        let scope_one = id("fixture-scope", Scope::new);
        let scope_two = id("fixture-scope-two", Scope::new);
        store.upsert_connection(connection()).unwrap();
        store
            .start_sync_run(run(SyncRunStatus::Running, None))
            .unwrap();

        store
            .commit_sync(SyncCommit {
                sync_run: run(SyncRunStatus::Succeeded, Some(timestamp_at(3))),
                resources: Vec::new(),
                resource_versions: Vec::new(),
                relations: Vec::new(),
                relation_versions: Vec::new(),
                changes: Vec::new(),
                cursor_after: Some(id("cursor-after", SyncCursor::new)),
                missing_evidence: Some(missing_state(scope_one.as_str(), "fixture-resource", 1)),
            })
            .unwrap();

        assert_eq!(
            store
                .missing_evidence_state(&connection_id, &scope_one)
                .unwrap(),
            Some(missing_state(scope_one.as_str(), "fixture-resource", 1,))
        );
        assert_eq!(
            store
                .missing_evidence_state(&connection_id, &scope_two)
                .unwrap(),
            None
        );

        let second_run_id = id("fixture-run-2", SyncRunId::new);
        let mut second_running = run(SyncRunStatus::Running, None);
        second_running.sync_run_id = second_run_id.clone();
        second_running.cursor_before = Some(id("cursor-after", SyncCursor::new));
        second_running.cursor_after = Some(id("cursor-second", SyncCursor::new));
        store.start_sync_run(second_running).unwrap();

        let mut second_finished = run(SyncRunStatus::Succeeded, Some(timestamp_at(5)));
        second_finished.sync_run_id = second_run_id;
        second_finished.cursor_before = Some(id("cursor-after", SyncCursor::new));
        second_finished.cursor_after = Some(id("cursor-second", SyncCursor::new));
        second_finished.coverage = SyncCoverage::AuthoritativeFull {
            scope: scope_two.clone(),
        };
        store
            .commit_sync(SyncCommit {
                sync_run: second_finished,
                resources: Vec::new(),
                resource_versions: Vec::new(),
                relations: Vec::new(),
                relation_versions: Vec::new(),
                changes: Vec::new(),
                cursor_after: Some(id("cursor-second", SyncCursor::new)),
                missing_evidence: Some(missing_state(scope_two.as_str(), "fixture-resource-2", 2)),
            })
            .unwrap();

        assert_eq!(
            store
                .missing_evidence_state(&connection_id, &scope_one)
                .unwrap(),
            Some(missing_state(scope_one.as_str(), "fixture-resource", 1,))
        );
        assert_eq!(
            store
                .missing_evidence_state(&connection_id, &scope_two)
                .unwrap(),
            Some(missing_state(scope_two.as_str(), "fixture-resource-2", 2,))
        );
    }

    #[test]
    fn projection_commit_is_readable_and_idempotent() {
        let (_directory, mut store) = store();
        store.upsert_connection(connection()).unwrap();
        store
            .start_sync_run(run(SyncRunStatus::Running, None))
            .unwrap();
        store
            .commit_sync(SyncCommit {
                sync_run: run(SyncRunStatus::Succeeded, Some(timestamp_at(3))),
                resources: vec![resource()],
                resource_versions: vec![version()],
                relations: Vec::new(),
                relation_versions: Vec::new(),
                changes: Vec::new(),
                cursor_after: Some(id("cursor-after", SyncCursor::new)),
                missing_evidence: None,
            })
            .unwrap();

        assert_eq!(
            store
                .get_resource(&id("fixture-resource", ResourceId::new))
                .unwrap(),
            Some(resource())
        );
        assert_eq!(
            store
                .sync_cursor(&id("fixture-connection", ConnectionId::new))
                .unwrap()
                .unwrap()
                .as_str(),
            "cursor-after"
        );

        let version_count: u32 = store
            .connection
            .query_row("SELECT count(*) FROM resource_versions", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(version_count, 1);

        let second_run_id = id("fixture-run-2", SyncRunId::new);
        let mut second_running = run(SyncRunStatus::Running, None);
        second_running.sync_run_id = second_run_id.clone();
        second_running.cursor_before = Some(id("cursor-after", SyncCursor::new));
        second_running.cursor_after = Some(id("cursor-second", SyncCursor::new));
        store.start_sync_run(second_running).unwrap();

        let mut second_finished = run(SyncRunStatus::Succeeded, Some(timestamp_at(5)));
        second_finished.sync_run_id = second_run_id.clone();
        second_finished.cursor_before = Some(id("cursor-after", SyncCursor::new));
        second_finished.cursor_after = Some(id("cursor-second", SyncCursor::new));
        let mut unchanged_resource = resource();
        unchanged_resource.last_seen_at = timestamp_at(4);
        unchanged_resource.last_sync_run_id = second_run_id;
        store
            .commit_sync(SyncCommit {
                sync_run: second_finished,
                resources: vec![unchanged_resource],
                resource_versions: Vec::new(),
                relations: Vec::new(),
                relation_versions: Vec::new(),
                changes: Vec::new(),
                cursor_after: Some(id("cursor-second", SyncCursor::new)),
                missing_evidence: None,
            })
            .unwrap();

        let version_count_after_unchanged: u32 = store
            .connection
            .query_row("SELECT count(*) FROM resource_versions", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(version_count_after_unchanged, 1);
    }

    #[test]
    fn latest_relation_version_fingerprint_is_deterministic() {
        let (_directory, mut store) = store();
        store.upsert_connection(connection()).unwrap();
        store
            .start_sync_run(run(SyncRunStatus::Running, None))
            .unwrap();
        store
            .commit_sync(SyncCommit {
                sync_run: run(SyncRunStatus::Succeeded, Some(timestamp_at(3))),
                resources: vec![resource()],
                resource_versions: vec![version()],
                relations: vec![relation("fixture-run")],
                relation_versions: vec![relation_version(
                    "fixture-relation-version-1",
                    "relation-fingerprint-1",
                    "fixture-run",
                    3,
                )],
                changes: Vec::new(),
                cursor_after: Some(id("cursor-after", SyncCursor::new)),
                missing_evidence: None,
            })
            .unwrap();

        let relation_id = id("fixture-relation", RelationId::new);
        assert_eq!(
            store
                .latest_relation_version_fingerprint(&relation_id)
                .unwrap()
                .unwrap()
                .as_str(),
            "relation-fingerprint-1"
        );

        let second_run_id = id("fixture-run-2", SyncRunId::new);
        let mut second_running = run(SyncRunStatus::Running, None);
        second_running.sync_run_id = second_run_id.clone();
        second_running.cursor_before = Some(id("cursor-after", SyncCursor::new));
        second_running.cursor_after = Some(id("cursor-second", SyncCursor::new));
        store.start_sync_run(second_running).unwrap();
        let mut second_finished = run(SyncRunStatus::Succeeded, Some(timestamp_at(5)));
        second_finished.sync_run_id = second_run_id;
        second_finished.cursor_before = Some(id("cursor-after", SyncCursor::new));
        second_finished.cursor_after = Some(id("cursor-second", SyncCursor::new));
        store
            .commit_sync(SyncCommit {
                sync_run: second_finished,
                resources: Vec::new(),
                resource_versions: Vec::new(),
                relations: vec![relation("fixture-run-2")],
                relation_versions: vec![relation_version(
                    "fixture-relation-version-2",
                    "relation-fingerprint-2",
                    "fixture-run-2",
                    5,
                )],
                changes: Vec::new(),
                cursor_after: Some(id("cursor-second", SyncCursor::new)),
                missing_evidence: None,
            })
            .unwrap();

        assert_eq!(
            store
                .latest_relation_version_fingerprint(&relation_id)
                .unwrap()
                .unwrap()
                .as_str(),
            "relation-fingerprint-2"
        );
    }

    #[test]
    fn failed_transaction_does_not_advance_cursor_or_expose_resource() {
        let (_directory, mut store) = store();
        store.upsert_connection(connection()).unwrap();
        store
            .start_sync_run(run(SyncRunStatus::Running, None))
            .unwrap();
        store
            .connection
            .execute(
                "INSERT INTO connector_state(connection_id, sync_cursor, consecutive_missing_json) VALUES (?1, ?2, ?3)",
                params![
                    "fixture-connection",
                    "cursor-before",
                    r#"{"fixture-scope":{"fixture-resource":1}}"#,
                ],
            )
            .unwrap();

        let bad_relation = Relation {
            relation_id: id("fixture-relation", RelationId::new),
            source_resource_id: id("fixture-resource", ResourceId::new),
            target_resource_id: id("missing-target", ResourceId::new),
            kind: RelationKind::new("fixture.depends_on").unwrap(),
            evidence_key: id("fixture-evidence", EvidenceKey::new),
            evidence: RelationEvidence::Provider {
                connection_id: id("fixture-connection", ConnectionId::new),
                sync_run_id: id("fixture-run", SyncRunId::new),
                field_path: id("attributes.target", FieldPath::new),
            },
            first_seen_at: timestamp_at(2),
            last_seen_at: timestamp_at(2),
            lifecycle: Lifecycle::Active,
        };
        let result = store.commit_sync(SyncCommit {
            sync_run: run(SyncRunStatus::Succeeded, Some(timestamp_at(3))),
            resources: vec![resource()],
            resource_versions: vec![version()],
            relations: vec![bad_relation],
            relation_versions: Vec::new(),
            changes: Vec::new(),
            cursor_after: Some(id("cursor-after", SyncCursor::new)),
            missing_evidence: Some(missing_state("fixture-scope", "fixture-resource", 2)),
        });

        assert!(result.is_err());
        assert!(
            store
                .get_resource(&id("fixture-resource", ResourceId::new))
                .unwrap()
                .is_none()
        );
        assert_eq!(
            store
                .sync_cursor(&id("fixture-connection", ConnectionId::new))
                .unwrap()
                .unwrap()
                .as_str(),
            "cursor-before"
        );
        assert_eq!(
            store
                .missing_evidence_state(
                    &id("fixture-connection", ConnectionId::new),
                    &id("fixture-scope", Scope::new),
                )
                .unwrap(),
            Some(missing_state("fixture-scope", "fixture-resource", 1,))
        );
    }

    #[test]
    fn recovery_marks_only_running_syncs_interrupted() {
        let (_directory, mut store) = store();
        store.upsert_connection(connection()).unwrap();
        store
            .start_sync_run(run(SyncRunStatus::Running, None))
            .unwrap();

        assert_eq!(
            store
                .mark_running_syncs_interrupted(timestamp_at(9))
                .unwrap(),
            1
        );
        let recovered = store
            .get_sync_run(&id("fixture-run", SyncRunId::new))
            .unwrap()
            .unwrap();
        assert_eq!(recovered.status, SyncRunStatus::Interrupted);
        assert_eq!(recovered.finished_at, Some(timestamp_at(9)));
    }
}
