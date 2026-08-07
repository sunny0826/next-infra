use crate::{Store, StoreError};
use next_infra_core::*;
use rusqlite::{OptionalExtension, Row, Transaction, params};
use serde::{Serialize, de::DeserializeOwned};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ConnectionPurgeSummary {
    pub resources: u64,
    pub relations: u64,
    pub resource_versions: u64,
    pub relation_versions: u64,
    pub changes: u64,
    pub bindings: u64,
    pub sync_runs: u64,
}

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
                "SELECT sync_run_id, connection_id, mode, trigger, started_at, finished_at, status, coverage_json, cursor_before, cursor_after, counts_json, errors_json, warnings_json FROM sync_runs WHERE sync_run_id = ?1",
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

impl Store {
    pub fn preview_connection_purge(
        &self,
        connection_id: &ConnectionId,
    ) -> Result<Option<ConnectionPurgeSummary>, StoreError> {
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(StoreError::Sqlite)?;
        let summary = connection_purge_summary(&transaction, connection_id)?;
        transaction.commit().map_err(StoreError::Sqlite)?;
        Ok(summary)
    }

    pub fn purge_connection(
        &mut self,
        connection_id: &ConnectionId,
    ) -> Result<ConnectionPurgeSummary, StoreError> {
        let transaction = self.connection.transaction().map_err(StoreError::Sqlite)?;
        let summary = connection_purge_summary(&transaction, connection_id)?
            .ok_or_else(|| StoreError::Contract("connection does not exist for purge".into()))?;

        transaction
            .execute(
                "DELETE FROM inference_outputs WHERE relation_id IN (SELECT relation_id FROM relations WHERE source_resource_id IN (SELECT resource_id FROM resources WHERE connection_id = ?1) OR target_resource_id IN (SELECT resource_id FROM resources WHERE connection_id = ?1))",
                params![connection_id.as_str()],
            )
            .map_err(StoreError::Sqlite)?;
        transaction
            .execute(
                "DELETE FROM relation_versions WHERE relation_id IN (SELECT relation_id FROM relations WHERE source_resource_id IN (SELECT resource_id FROM resources WHERE connection_id = ?1) OR target_resource_id IN (SELECT resource_id FROM resources WHERE connection_id = ?1))",
                params![connection_id.as_str()],
            )
            .map_err(StoreError::Sqlite)?;
        transaction
            .execute(
                "DELETE FROM changes WHERE (subject_type = 'resource' AND subject_id IN (SELECT resource_id FROM resources WHERE connection_id = ?1)) OR (subject_type = 'relation' AND subject_id IN (SELECT relation_id FROM relations WHERE source_resource_id IN (SELECT resource_id FROM resources WHERE connection_id = ?1) OR target_resource_id IN (SELECT resource_id FROM resources WHERE connection_id = ?1))) OR (subject_type = 'binding' AND subject_id IN (SELECT binding_id FROM bindings WHERE source_resource_id IN (SELECT resource_id FROM resources WHERE connection_id = ?1) OR target_resource_id IN (SELECT resource_id FROM resources WHERE connection_id = ?1)))",
                params![connection_id.as_str()],
            )
            .map_err(StoreError::Sqlite)?;
        transaction
            .execute(
                "DELETE FROM bindings WHERE source_resource_id IN (SELECT resource_id FROM resources WHERE connection_id = ?1) OR target_resource_id IN (SELECT resource_id FROM resources WHERE connection_id = ?1)",
                params![connection_id.as_str()],
            )
            .map_err(StoreError::Sqlite)?;
        transaction
            .execute(
                "DELETE FROM relations WHERE source_resource_id IN (SELECT resource_id FROM resources WHERE connection_id = ?1) OR target_resource_id IN (SELECT resource_id FROM resources WHERE connection_id = ?1)",
                params![connection_id.as_str()],
            )
            .map_err(StoreError::Sqlite)?;
        transaction
            .execute(
                "DELETE FROM resource_versions WHERE resource_id IN (SELECT resource_id FROM resources WHERE connection_id = ?1)",
                params![connection_id.as_str()],
            )
            .map_err(StoreError::Sqlite)?;
        transaction
            .execute(
                "DELETE FROM resources WHERE connection_id = ?1",
                params![connection_id.as_str()],
            )
            .map_err(StoreError::Sqlite)?;
        transaction
            .execute(
                "DELETE FROM connector_state WHERE connection_id = ?1",
                params![connection_id.as_str()],
            )
            .map_err(StoreError::Sqlite)?;
        transaction
            .execute(
                "DELETE FROM sync_runs WHERE connection_id = ?1",
                params![connection_id.as_str()],
            )
            .map_err(StoreError::Sqlite)?;
        let removed = transaction
            .execute(
                "DELETE FROM connections WHERE connection_id = ?1",
                params![connection_id.as_str()],
            )
            .map_err(StoreError::Sqlite)?;
        if removed != 1 {
            return Err(StoreError::Contract(
                "connection disappeared during purge".into(),
            ));
        }
        bump_projection_metadata(&transaction)?;
        transaction.commit().map_err(StoreError::Sqlite)?;
        Ok(summary)
    }
}

fn connection_purge_summary(
    connection: &rusqlite::Connection,
    connection_id: &ConnectionId,
) -> Result<Option<ConnectionPurgeSummary>, StoreError> {
    let exists = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM connections WHERE connection_id = ?1)",
            params![connection_id.as_str()],
            |row| row.get::<_, bool>(0),
        )
        .map_err(StoreError::Sqlite)?;
    if !exists {
        return Ok(None);
    }
    Ok(Some(ConnectionPurgeSummary {
        resources: purge_count(
            connection,
            "SELECT COUNT(*) FROM resources WHERE connection_id = ?1",
            connection_id,
        )?,
        relations: purge_count(
            connection,
            "SELECT COUNT(*) FROM relations WHERE source_resource_id IN (SELECT resource_id FROM resources WHERE connection_id = ?1) OR target_resource_id IN (SELECT resource_id FROM resources WHERE connection_id = ?1)",
            connection_id,
        )?,
        resource_versions: purge_count(
            connection,
            "SELECT COUNT(*) FROM resource_versions WHERE resource_id IN (SELECT resource_id FROM resources WHERE connection_id = ?1)",
            connection_id,
        )?,
        relation_versions: purge_count(
            connection,
            "SELECT COUNT(*) FROM relation_versions WHERE relation_id IN (SELECT relation_id FROM relations WHERE source_resource_id IN (SELECT resource_id FROM resources WHERE connection_id = ?1) OR target_resource_id IN (SELECT resource_id FROM resources WHERE connection_id = ?1))",
            connection_id,
        )?,
        changes: purge_count(
            connection,
            "SELECT COUNT(*) FROM changes WHERE (subject_type = 'resource' AND subject_id IN (SELECT resource_id FROM resources WHERE connection_id = ?1)) OR (subject_type = 'relation' AND subject_id IN (SELECT relation_id FROM relations WHERE source_resource_id IN (SELECT resource_id FROM resources WHERE connection_id = ?1) OR target_resource_id IN (SELECT resource_id FROM resources WHERE connection_id = ?1))) OR (subject_type = 'binding' AND subject_id IN (SELECT binding_id FROM bindings WHERE source_resource_id IN (SELECT resource_id FROM resources WHERE connection_id = ?1) OR target_resource_id IN (SELECT resource_id FROM resources WHERE connection_id = ?1)))",
            connection_id,
        )?,
        bindings: purge_count(
            connection,
            "SELECT COUNT(*) FROM bindings WHERE source_resource_id IN (SELECT resource_id FROM resources WHERE connection_id = ?1) OR target_resource_id IN (SELECT resource_id FROM resources WHERE connection_id = ?1)",
            connection_id,
        )?,
        sync_runs: purge_count(
            connection,
            "SELECT COUNT(*) FROM sync_runs WHERE connection_id = ?1",
            connection_id,
        )?,
    }))
}

fn purge_count(
    connection: &rusqlite::Connection,
    sql: &str,
    connection_id: &ConnectionId,
) -> Result<u64, StoreError> {
    let count: i64 = connection
        .query_row(sql, params![connection_id.as_str()], |row| row.get(0))
        .map_err(StoreError::Sqlite)?;
    u64::try_from(count).map_err(|_| StoreError::Contract("negative purge count".into()))
}

impl BindingStore for Store {
    fn get_binding(&self, id: &BindingId) -> Result<Option<Binding>, Self::Error> {
        self.connection
            .query_row(
                "SELECT binding_id, source_resource_id, target_resource_id, kind, status, created_at, updated_at FROM bindings WHERE binding_id = ?1",
                params![id.as_str()],
                read_binding,
            )
            .optional()
            .map_err(StoreError::Sqlite)
    }

    fn list_bindings(&self) -> Result<Vec<Binding>, Self::Error> {
        let mut statement = self
            .connection
            .prepare("SELECT binding_id, source_resource_id, target_resource_id, kind, status, created_at, updated_at FROM bindings ORDER BY binding_id")
            .map_err(StoreError::Sqlite)?;
        statement
            .query_map([], read_binding)
            .map_err(StoreError::Sqlite)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::Sqlite)
    }

    fn commit_binding(&mut self, commit: BindingCommit) -> Result<(), Self::Error> {
        let transaction = self.connection.transaction().map_err(StoreError::Sqlite)?;
        transaction
            .execute(
                "INSERT INTO bindings(binding_id, source_resource_id, target_resource_id, kind, status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) ON CONFLICT(binding_id) DO UPDATE SET source_resource_id = excluded.source_resource_id, target_resource_id = excluded.target_resource_id, kind = excluded.kind, status = excluded.status, updated_at = excluded.updated_at",
                params![
                    commit.binding.binding_id.as_str(),
                    commit.binding.source_resource_id.as_str(),
                    commit.binding.target_resource_id.as_str(),
                    commit.binding.kind.as_str(),
                    enum_text(&commit.binding.status)?,
                    commit.binding.created_at.unix_millis(),
                    commit.binding.updated_at.unix_millis(),
                ],
            )
            .map_err(StoreError::Sqlite)?;
        for relation in &commit.relations {
            upsert_relation(&transaction, relation)?;
        }
        for version in &commit.relation_versions {
            insert_relation_version(&transaction, version)?;
        }
        for change in &commit.changes {
            insert_change(&transaction, change)?;
        }
        transaction
            .execute(
                "UPDATE projection_metadata SET committed_revision = committed_revision + 1, committed_at = ?1 WHERE singleton_id = 1",
                params![commit.binding.updated_at.unix_millis()],
            )
            .map_err(StoreError::Sqlite)?;
        transaction.commit().map_err(StoreError::Sqlite)
    }
}

impl InferenceStore for Store {
    fn resource_version_exists(&self, id: &ResourceVersionId) -> Result<bool, Self::Error> {
        self.connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM resource_versions WHERE version_id = ?1)",
                params![id.as_str()],
                |row| row.get(0),
            )
            .map_err(StoreError::Sqlite)
    }

    fn relation_version_exists(&self, id: &RelationVersionId) -> Result<bool, Self::Error> {
        self.connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM relation_versions WHERE relation_version_id = ?1)",
                params![id.as_str()],
                |row| row.get(0),
            )
            .map_err(StoreError::Sqlite)
    }

    fn inferred_relations_for_rule(
        &self,
        rule_version: &RuleVersion,
    ) -> Result<Vec<Relation>, Self::Error> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT relation_id, source_resource_id, target_resource_id, kind, evidence_key, evidence_json, first_seen_at, last_seen_at, lifecycle FROM relations WHERE evidence_type = 'inferred' AND json_extract(evidence_json, '$.rule_version') = ?1 ORDER BY relation_id",
            )
            .map_err(StoreError::Sqlite)?;
        statement
            .query_map(params![rule_version.as_str()], read_relation)
            .map_err(StoreError::Sqlite)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::Sqlite)
    }

    fn commit_inference(&mut self, commit: InferenceCommit) -> Result<(), Self::Error> {
        let transaction = self.connection.transaction().map_err(StoreError::Sqlite)?;
        transaction
            .execute(
                "INSERT INTO inference_runs(inference_run_id, rule_version, started_at, finished_at, status, input_resource_version_ids_json, input_relation_version_ids_json, summary_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    commit.run.inference_run_id.as_str(),
                    commit.run.rule_version.as_str(),
                    commit.run.started_at.unix_millis(),
                    timestamp_option(commit.run.finished_at),
                    enum_text(&commit.run.status)?,
                    json_text(&commit.run.input_resource_version_ids)?,
                    json_text(&commit.run.input_relation_version_ids)?,
                    json_text(&serde_json::json!({ "output_relation_ids": commit.run.output_relation_ids }))?,
                ],
            )
            .map_err(StoreError::Sqlite)?;
        for relation in &commit.relations {
            upsert_relation(&transaction, relation)?;
        }
        for version in &commit.relation_versions {
            insert_relation_version(&transaction, version)?;
        }
        for change in &commit.changes {
            insert_change(&transaction, change)?;
        }
        for relation_id in &commit.run.output_relation_ids {
            let evidence_key = commit
                .relations
                .iter()
                .find(|relation| &relation.relation_id == relation_id)
                .map(|relation| relation.evidence_key.as_str())
                .ok_or_else(|| {
                    StoreError::Contract(
                        "inference output relation is missing from the commit".into(),
                    )
                })?;
            transaction
                .execute(
                    "INSERT INTO inference_outputs(inference_run_id, relation_id, evidence_key) VALUES (?1, ?2, ?3)",
                    params![commit.run.inference_run_id.as_str(), relation_id.as_str(), evidence_key],
                )
                .map_err(StoreError::Sqlite)?;
        }
        transaction
            .execute(
                "UPDATE projection_metadata SET committed_revision = committed_revision + 1, committed_at = ?1 WHERE singleton_id = 1",
                params![commit.run.finished_at.unwrap_or(commit.run.started_at).unix_millis()],
            )
            .map_err(StoreError::Sqlite)?;
        transaction.commit().map_err(StoreError::Sqlite)
    }
}

impl StoreWriter for Store {
    type Error = StoreError;

    fn upsert_connection(&mut self, connection: Connection) -> Result<(), Self::Error> {
        let config = serde_json::to_string(&connection.config).map_err(StoreError::Json)?;
        let secret_ref = connection.secret_ref.as_ref().map(json_text).transpose()?;
        let health = enum_text(&connection.health)?;
        let transaction = self.connection.transaction().map_err(StoreError::Sqlite)?;
        transaction
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
                    health,
                    timestamp_option(connection.last_success_at),
                    timestamp_option(connection.last_attempt_at),
                    connection.config_schema_version.get(),
                    timestamp_option(connection.deleted_at),
                ],
            )
            .map_err(StoreError::Sqlite)?;
        bump_projection_metadata(&transaction)?;
        transaction.commit().map_err(StoreError::Sqlite)?;
        Ok(())
    }

    fn start_sync_run(&mut self, sync_run: SyncRun) -> Result<(), Self::Error> {
        if sync_run.status != SyncRunStatus::Running || sync_run.finished_at.is_some() {
            return Err(StoreError::Contract(
                "start_sync_run requires running status without finished_at".into(),
            ));
        }
        let transaction = self.connection.transaction().map_err(StoreError::Sqlite)?;
        insert_sync_run(&transaction, &sync_run)?;
        bump_projection_metadata(&transaction)?;
        transaction.commit().map_err(StoreError::Sqlite)
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
        bump_projection_metadata(&transaction)?;
        transaction.commit().map_err(StoreError::Sqlite)?;
        Ok(result)
    }

    fn mark_running_syncs_interrupted(&mut self, at: Timestamp) -> Result<usize, Self::Error> {
        let transaction = self.connection.transaction().map_err(StoreError::Sqlite)?;
        let updated = transaction
            .execute(
                "UPDATE sync_runs SET status = 'interrupted', finished_at = ?1 WHERE status = 'running'",
                params![at.unix_millis()],
            )
            .map_err(StoreError::Sqlite)?;
        if updated > 0 {
            bump_projection_metadata(&transaction)?;
        }
        transaction.commit().map_err(StoreError::Sqlite)?;
        Ok(updated)
    }
}

fn bump_projection_metadata(connection: &rusqlite::Connection) -> Result<(), StoreError> {
    let updated = connection
        .execute(
            "UPDATE projection_metadata SET committed_revision = committed_revision + 1, committed_at = CAST(unixepoch('subsec') * 1000 AS INTEGER) WHERE singleton_id = 1",
            [],
        )
        .map_err(StoreError::Sqlite)?;
    if updated == 1 {
        Ok(())
    } else {
        Err(StoreError::Contract(
            "projection metadata singleton is missing".into(),
        ))
    }
}

fn insert_sync_run(connection: &rusqlite::Connection, run: &SyncRun) -> Result<(), StoreError> {
    connection
        .execute(
            "INSERT INTO sync_runs(sync_run_id, connection_id, mode, trigger, started_at, finished_at, status, coverage_json, cursor_before, cursor_after, counts_json, errors_json, warnings_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                run.sync_run_id.as_str(), run.connection_id.as_str(), enum_text(&run.mode)?, enum_text(&run.trigger)?, run.started_at.unix_millis(), timestamp_option(run.finished_at), enum_text(&run.status)?, json_text(&run.coverage)?, run.cursor_before.as_ref().map(SyncCursor::as_str), run.cursor_after.as_ref().map(SyncCursor::as_str), json_text(&run.counts)?, json_text(&run.errors)?, json_text(&run.warnings)?,
            ],
        )
        .map_err(StoreError::Sqlite)?;
    Ok(())
}

fn complete_sync_run(transaction: &Transaction<'_>, run: &SyncRun) -> Result<(), StoreError> {
    let updated = transaction
        .execute(
            "UPDATE sync_runs SET finished_at=?2, status=?3, coverage_json=?4, cursor_after=?5, counts_json=?6, errors_json=?7, warnings_json=?8 WHERE sync_run_id=?1 AND connection_id=?9",
            params![run.sync_run_id.as_str(), timestamp_option(run.finished_at), enum_text(&run.status)?, json_text(&run.coverage)?, run.cursor_after.as_ref().map(SyncCursor::as_str), json_text(&run.counts)?, json_text(&run.errors)?, json_text(&run.warnings)?, run.connection_id.as_str()],
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
        ChangeSubject::Binding { binding_id } => ("binding", binding_id.as_str()),
    };
    transaction.execute(
        "INSERT OR IGNORE INTO changes(change_id, subject_type, subject_id, observed_at, fields_json, origin_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![change.change_id.as_str(), subject_type, subject_id, change.observed_at.unix_millis(), json_text(&change.fields)?, json_text(&change.origin)?],
    ).map_err(StoreError::Sqlite)
}

pub(crate) fn read_connection(row: &Row<'_>) -> rusqlite::Result<Connection> {
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

pub(crate) fn read_resource(row: &Row<'_>) -> rusqlite::Result<Resource> {
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

pub(crate) fn read_relation(row: &Row<'_>) -> rusqlite::Result<Relation> {
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

pub(crate) fn read_sync_run(row: &Row<'_>) -> rusqlite::Result<SyncRun> {
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
        warnings: json(row.get(12)?)?,
    })
}

pub(crate) fn read_change(row: &Row<'_>) -> rusqlite::Result<Change> {
    let subject_type: String = row.get(1)?;
    let subject_id: String = row.get(2)?;
    let subject = match subject_type.as_str() {
        "resource" => ChangeSubject::Resource {
            resource_id: wrapped(subject_id)?,
        },
        "relation" => ChangeSubject::Relation {
            relation_id: wrapped(subject_id)?,
        },
        "binding" => ChangeSubject::Binding {
            binding_id: wrapped(subject_id)?,
        },
        _ => {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                1,
                rusqlite::types::Type::Text,
                "unknown change subject type".into(),
            ));
        }
    };
    Ok(Change {
        change_id: wrapped(row.get(0)?)?,
        subject,
        observed_at: timestamp(row.get(3)?)?,
        fields: json(row.get(4)?)?,
        origin: json(row.get(5)?)?,
    })
}

fn read_binding(row: &Row<'_>) -> rusqlite::Result<Binding> {
    Ok(Binding {
        binding_id: wrapped(row.get(0)?)?,
        source_resource_id: wrapped(row.get(1)?)?,
        target_resource_id: wrapped(row.get(2)?)?,
        kind: wrapped(row.get(3)?)?,
        status: enum_value(row.get(4)?)?,
        created_at: timestamp(row.get(5)?)?,
        updated_at: timestamp(row.get(6)?)?,
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

pub(crate) fn wrapped<T: DeserializeOwned>(value: String) -> rusqlite::Result<T> {
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
    use crate::{FreshnessCutoffs, RecentChangesProjectionPlan, ResourceProjectionPlan};
    use serde_json::json;
    use std::collections::{BTreeMap, BTreeSet};
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
            warnings: Vec::new(),
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

    fn projection_cutoffs() -> BTreeMap<ConnectionId, FreshnessCutoffs> {
        BTreeMap::from([(
            id("fixture-connection", ConnectionId::new),
            FreshnessCutoffs {
                fresh_after_millis: 3,
                expired_after_millis: 2,
            },
        )])
    }

    fn query_projection_store() -> (TempDir, Store) {
        let (directory, mut store) = store();
        let mut query_connection = connection();
        query_connection.secret_ref = Some(secret_ref());
        store.upsert_connection(query_connection).unwrap();
        store
            .start_sync_run(run(SyncRunStatus::Running, None))
            .unwrap();

        let mut alpha = resource();
        alpha.resource_id = id("fixture-resource-alpha", ResourceId::new);
        alpha.external_id = id("external-alpha", ExternalId::new);
        alpha.name = "alpha".into();
        alpha.display_name = "Fixture Compute Alpha".into();
        alpha.kind = ResourceKind::new("fixture.compute").unwrap();
        alpha.last_seen_at = timestamp_at(4);
        alpha.labels = BTreeMap::from([(LabelKey::new("fixture.tier").unwrap(), "compute".into())]);

        let mut beta = resource();
        beta.resource_id = id("fixture-resource-beta", ResourceId::new);
        beta.external_id = id("external-beta", ExternalId::new);
        beta.name = "beta".into();
        beta.display_name = "Fixture Database Beta".into();
        beta.kind = ResourceKind::new("fixture.database").unwrap();
        beta.last_seen_at = timestamp_at(2);
        beta.labels = BTreeMap::from([(LabelKey::new("fixture.tier").unwrap(), "database".into())]);

        let mut gamma = resource();
        gamma.resource_id = id("fixture-resource-gamma", ResourceId::new);
        gamma.external_id = id("external-gamma", ExternalId::new);
        gamma.name = "gamma".into();
        gamma.display_name = "Fixture Worker Gamma".into();
        gamma.kind = ResourceKind::new("fixture.worker").unwrap();
        gamma.last_seen_at = timestamp_at(1);
        gamma.health = ResourceHealth::Degraded;
        gamma.labels = BTreeMap::from([(LabelKey::new("fixture.tier").unwrap(), "worker".into())]);

        let relation_id = id("fixture-relation-alpha-beta", RelationId::new);
        let mut alpha_beta = relation("fixture-run");
        alpha_beta.relation_id = relation_id.clone();
        alpha_beta.source_resource_id = alpha.resource_id.clone();
        alpha_beta.target_resource_id = beta.resource_id.clone();
        alpha_beta.evidence_key = id("fixture-evidence-alpha-beta", EvidenceKey::new);

        let resource_change = Change {
            change_id: id("fixture-change-resource", ChangeId::new),
            subject: ChangeSubject::Resource {
                resource_id: alpha.resource_id.clone(),
            },
            observed_at: timestamp_at(4),
            fields: vec![FieldChange {
                path: FieldPath::new("attributes.state").unwrap(),
                before: Some(json!("pending")),
                after: Some(json!("ready")),
            }],
            origin: OriginRef::SyncRun {
                sync_run_id: id("fixture-run", SyncRunId::new),
            },
        };
        let relation_change = Change {
            change_id: id("fixture-change-relation", ChangeId::new),
            subject: ChangeSubject::Relation { relation_id },
            observed_at: timestamp_at(3),
            fields: vec![FieldChange {
                path: FieldPath::new("lifecycle").unwrap(),
                before: None,
                after: Some(json!("active")),
            }],
            origin: OriginRef::SyncRun {
                sync_run_id: id("fixture-run", SyncRunId::new),
            },
        };

        store
            .commit_sync(SyncCommit {
                sync_run: run(SyncRunStatus::Succeeded, Some(timestamp_at(5))),
                resources: vec![alpha, beta, gamma],
                resource_versions: Vec::new(),
                relations: vec![alpha_beta],
                relation_versions: Vec::new(),
                changes: vec![resource_change, relation_change],
                cursor_after: Some(id("cursor-after", SyncCursor::new)),
                missing_evidence: None,
            })
            .unwrap();
        (directory, store)
    }

    #[test]
    fn connection_purge_removes_only_the_selected_connection_snapshot() {
        let (_directory, mut store) = query_projection_store();
        let connection_id = id("fixture-connection", ConnectionId::new);
        let mut retained = connection();
        retained.connection_id = id("retained-connection", ConnectionId::new);
        retained.display_name = "Retained connection".into();
        store.upsert_connection(retained.clone()).unwrap();

        let summary = store
            .preview_connection_purge(&connection_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            summary,
            ConnectionPurgeSummary {
                resources: 3,
                relations: 1,
                resource_versions: 0,
                relation_versions: 0,
                changes: 2,
                bindings: 0,
                sync_runs: 1,
            }
        );
        let revision_before = store.projection_metadata().unwrap().committed_revision;

        assert_eq!(store.purge_connection(&connection_id).unwrap(), summary);
        assert_eq!(store.get_connection(&connection_id).unwrap(), None);
        assert_eq!(
            store.get_connection(&retained.connection_id).unwrap(),
            Some(retained)
        );
        assert!(
            store
                .query_connections()
                .unwrap()
                .body
                .iter()
                .all(|connection| { connection.connection_id.as_str() != connection_id.as_str() })
        );
        assert_eq!(
            store.projection_metadata().unwrap().committed_revision,
            revision_before + 1
        );
        assert_eq!(
            store.preview_connection_purge(&connection_id).unwrap(),
            None
        );
        let foreign_key_violation: Option<String> = store
            .connection
            .query_row("PRAGMA foreign_key_check", [], |row| row.get(0))
            .optional()
            .unwrap();
        assert_eq!(foreign_key_violation, None);
    }

    fn topology_store() -> (TempDir, Store, Resource, Resource) {
        let (directory, mut store) = store();
        store.upsert_connection(connection()).unwrap();
        store
            .start_sync_run(run(SyncRunStatus::Running, None))
            .unwrap();

        let mut source = resource();
        source.resource_id = id("fixture-resource-source", ResourceId::new);
        source.external_id = id("external-source", ExternalId::new);
        source.name = "source".into();
        source.display_name = "Fixture Source".into();

        let mut target = resource();
        target.resource_id = id("fixture-resource-target", ResourceId::new);
        target.external_id = id("external-target", ExternalId::new);
        target.name = "target".into();
        target.display_name = "Fixture Target".into();

        store
            .commit_sync(SyncCommit {
                sync_run: run(SyncRunStatus::Succeeded, Some(timestamp_at(3))),
                resources: vec![source.clone(), target.clone()],
                resource_versions: Vec::new(),
                relations: Vec::new(),
                relation_versions: Vec::new(),
                changes: Vec::new(),
                cursor_after: Some(id("cursor-after", SyncCursor::new)),
                missing_evidence: None,
            })
            .unwrap();
        (directory, store, source, target)
    }

    fn binding(source: &Resource, target: &Resource) -> Binding {
        Binding {
            binding_id: id("fixture-binding", BindingId::new),
            source_resource_id: source.resource_id.clone(),
            target_resource_id: target.resource_id.clone(),
            kind: RelationKind::new("fixture.configured").unwrap(),
            status: BindingStatus::Active,
            created_at: timestamp_at(10),
            updated_at: timestamp_at(11),
        }
    }

    fn configured_relation(binding: &Binding, lifecycle: Lifecycle, at: i64) -> Relation {
        Relation {
            relation_id: id("fixture-configured-relation", RelationId::new),
            source_resource_id: binding.source_resource_id.clone(),
            target_resource_id: binding.target_resource_id.clone(),
            kind: binding.kind.clone(),
            evidence_key: id("fixture-configured-evidence", EvidenceKey::new),
            evidence: RelationEvidence::Configured {
                binding_id: binding.binding_id.clone(),
            },
            first_seen_at: binding.created_at,
            last_seen_at: timestamp_at(at),
            lifecycle,
        }
    }

    fn configured_relation_version(
        relation: &Relation,
        version_id: &str,
        binding_id: &BindingId,
        observed_at: i64,
    ) -> RelationVersion {
        RelationVersion {
            relation_version_id: id(version_id, RelationVersionId::new),
            relation_id: relation.relation_id.clone(),
            observed_at: timestamp_at(observed_at),
            normalized_snapshot: json!({"relation": "configured"}),
            fingerprint: id("fixture-configured-fingerprint", Fingerprint::new),
            schema_version: SchemaVersion::new(1).unwrap(),
            origin: OriginRef::Binding {
                binding_id: binding_id.clone(),
            },
        }
    }

    fn binding_change(binding: &Binding, change_id: &str, observed_at: i64) -> Change {
        Change {
            change_id: id(change_id, ChangeId::new),
            subject: ChangeSubject::Binding {
                binding_id: binding.binding_id.clone(),
            },
            observed_at: timestamp_at(observed_at),
            fields: vec![FieldChange {
                path: FieldPath::new("status").unwrap(),
                before: None,
                after: Some(json!("active")),
            }],
            origin: OriginRef::Binding {
                binding_id: binding.binding_id.clone(),
            },
        }
    }

    fn inferred_relation(
        relation_id: &str,
        rule_version: &RuleVersion,
        source: &Resource,
        target: &Resource,
        lifecycle: Lifecycle,
        at: i64,
        provenance: (&[ResourceVersionId], &[RelationVersionId]),
    ) -> Relation {
        Relation {
            relation_id: id(relation_id, RelationId::new),
            source_resource_id: source.resource_id.clone(),
            target_resource_id: target.resource_id.clone(),
            kind: RelationKind::new("fixture.inferred").unwrap(),
            evidence_key: id(&format!("fixture-evidence-{relation_id}"), EvidenceKey::new),
            evidence: RelationEvidence::Inferred {
                rule_version: rule_version.clone(),
                input_resource_version_ids: provenance.0.to_vec(),
                input_relation_version_ids: provenance.1.to_vec(),
                confidence: Confidence::from_basis_points(8_750).unwrap(),
            },
            first_seen_at: timestamp_at(at),
            last_seen_at: timestamp_at(at),
            lifecycle,
        }
    }

    fn inference_provenance(
        relation: &Relation,
    ) -> (RuleVersion, Vec<ResourceVersionId>, Vec<RelationVersionId>) {
        match &relation.evidence {
            RelationEvidence::Inferred {
                rule_version,
                input_resource_version_ids,
                input_relation_version_ids,
                ..
            } => (
                rule_version.clone(),
                input_resource_version_ids.clone(),
                input_relation_version_ids.clone(),
            ),
            _ => panic!("fixture relation must be inferred"),
        }
    }

    fn inference_relation_version(
        relation: &Relation,
        version_id: &str,
        observed_at: i64,
    ) -> RelationVersion {
        let (rule_version, input_resource_version_ids, input_relation_version_ids) =
            inference_provenance(relation);
        RelationVersion {
            relation_version_id: id(version_id, RelationVersionId::new),
            relation_id: relation.relation_id.clone(),
            observed_at: timestamp_at(observed_at),
            normalized_snapshot: json!({"relation": relation.relation_id.as_str()}),
            fingerprint: id(
                &format!("fixture-fingerprint-{version_id}"),
                Fingerprint::new,
            ),
            schema_version: SchemaVersion::new(1).unwrap(),
            origin: OriginRef::Inference {
                rule_version,
                input_resource_version_ids,
                input_relation_version_ids,
            },
        }
    }

    fn inference_change(relation: &Relation, change_id: &str, observed_at: i64) -> Change {
        let (rule_version, input_resource_version_ids, input_relation_version_ids) =
            inference_provenance(relation);
        Change {
            change_id: id(change_id, ChangeId::new),
            subject: ChangeSubject::Relation {
                relation_id: relation.relation_id.clone(),
            },
            observed_at: timestamp_at(observed_at),
            fields: vec![FieldChange {
                path: FieldPath::new("relation.lifecycle").unwrap(),
                before: None,
                after: Some(json!(relation.lifecycle)),
            }],
            origin: OriginRef::Inference {
                rule_version,
                input_resource_version_ids,
                input_relation_version_ids,
            },
        }
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
        let revision_before_failure = store.projection_metadata().unwrap().committed_revision;
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
        assert_eq!(
            store.projection_metadata().unwrap().committed_revision,
            revision_before_failure
        );
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

    #[test]
    fn projection_revision_tracks_only_committed_query_visible_mutations() {
        let (_directory, mut store) = store();
        assert_eq!(store.projection_metadata().unwrap().committed_revision, 0);

        store.upsert_connection(connection()).unwrap();
        assert_eq!(store.projection_metadata().unwrap().committed_revision, 1);

        store
            .start_sync_run(run(SyncRunStatus::Running, None))
            .unwrap();
        assert_eq!(store.projection_metadata().unwrap().committed_revision, 2);

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
        assert_eq!(store.projection_metadata().unwrap().committed_revision, 3);

        assert_eq!(
            store
                .mark_running_syncs_interrupted(timestamp_at(4))
                .unwrap(),
            0
        );
        assert_eq!(store.projection_metadata().unwrap().committed_revision, 3);

        let mut second_run = run(SyncRunStatus::Running, None);
        second_run.sync_run_id = id("fixture-run-2", SyncRunId::new);
        store.start_sync_run(second_run).unwrap();
        assert_eq!(store.projection_metadata().unwrap().committed_revision, 4);

        assert_eq!(
            store
                .mark_running_syncs_interrupted(timestamp_at(5))
                .unwrap(),
            1
        );
        assert_eq!(store.projection_metadata().unwrap().committed_revision, 5);
    }

    #[test]
    fn bounded_resource_projection_filters_and_pages_stably() {
        let (_directory, store) = query_projection_store();
        let mut plan = ResourceProjectionPlan {
            query: None,
            kinds: BTreeSet::new(),
            connector_types: BTreeSet::new(),
            health: Vec::new(),
            freshness: Vec::new(),
            labels: BTreeMap::new(),
            cutoffs: projection_cutoffs(),
            limit: 2,
            after: None,
        };

        let first = store.query_resources(&plan).unwrap();
        assert_eq!(first.metadata, store.projection_metadata().unwrap());
        assert_eq!(first.body.items.len(), 2);
        assert_eq!(first.body.items[0].freshness, Freshness::Fresh);
        assert_eq!(first.body.items[1].freshness, Freshness::Stale);
        assert_eq!(
            first.body.next_after.as_deref(),
            Some("fixture-resource-beta")
        );

        plan.after = first.body.next_after;
        let second = store.query_resources(&plan).unwrap();
        assert_eq!(second.body.items.len(), 1);
        assert_eq!(second.body.items[0].freshness, Freshness::Expired);
        assert_eq!(second.body.next_after, None);

        plan.after = None;
        plan.limit = 10;
        plan.query = Some("database".into());
        plan.labels = BTreeMap::from([("fixture.tier".into(), "database".into())]);
        plan.freshness = vec![Freshness::Stale];
        let filtered = store.query_resources(&plan).unwrap();
        assert_eq!(filtered.body.items.len(), 1);
        assert_eq!(
            filtered.body.items[0].resource.resource_id.as_str(),
            "fixture-resource-beta"
        );
    }

    #[test]
    fn detail_frontier_change_sync_and_health_projections_share_revision() {
        let (_directory, store) = query_projection_store();
        let alpha = id("fixture-resource-alpha", ResourceId::new);
        let metadata = store.projection_metadata().unwrap();

        let detail = store.query_resource_detail(&alpha).unwrap();
        assert_eq!(detail.metadata, metadata);
        let detail = detail.body.unwrap();
        assert_eq!(detail.relations.len(), 1);
        assert_eq!(detail.recent_changes.len(), 2);
        assert!(!detail.relations_truncated);
        assert!(!detail.recent_changes_truncated);

        let frontier = store
            .query_relations_for_resources(&BTreeSet::from([alpha.clone()]), 10, None)
            .unwrap();
        assert_eq!(frontier.metadata, metadata);
        assert_eq!(frontier.body.items.len(), 1);

        let resources = store
            .query_resources_by_ids(&BTreeSet::from([
                alpha.clone(),
                id("fixture-resource-beta", ResourceId::new),
            ]))
            .unwrap();
        assert_eq!(resources.body.len(), 2);

        let mut changes_plan = RecentChangesProjectionPlan {
            since_millis: None,
            resource_id: Some(alpha),
            kinds: BTreeSet::new(),
            limit: 1,
            after: None,
        };
        let first_change = store.query_recent_changes(&changes_plan).unwrap();
        assert_eq!(first_change.body.items.len(), 1);
        assert!(first_change.body.next_after.is_some());
        changes_plan.after = first_change.body.next_after;
        let second_change = store.query_recent_changes(&changes_plan).unwrap();
        assert_eq!(second_change.body.items.len(), 1);
        assert_eq!(second_change.body.next_after, None);

        let sync = store
            .query_sync_status(&id("fixture-connection", ConnectionId::new), 10)
            .unwrap();
        assert_eq!(sync.metadata, metadata);
        assert_eq!(sync.body.unwrap().recent_runs.len(), 1);

        let health = store.query_health_summary(&projection_cutoffs()).unwrap();
        assert_eq!(health.metadata, metadata);
        assert_eq!(
            health.body.freshness,
            vec![
                (Freshness::Fresh, 1),
                (Freshness::Stale, 1),
                (Freshness::Expired, 1),
            ]
        );
        let connections = store.query_connections().unwrap().body;
        assert_eq!(connections.len(), 1);
        assert!(!format!("{connections:?}").contains("generation"));
    }

    #[test]
    fn relation_projections_enrich_authoritative_metadata_without_inventing_values() {
        let (_directory, mut store) = store();
        store.upsert_connection(connection()).unwrap();
        store
            .start_sync_run(run(SyncRunStatus::Running, None))
            .unwrap();

        let mut source = resource();
        source.resource_id = id("fixture-resource-source", ResourceId::new);
        source.external_id = id("external-source", ExternalId::new);
        let mut target = resource();
        target.resource_id = id("fixture-resource-target", ResourceId::new);
        target.external_id = id("external-target", ExternalId::new);

        let mut provider = relation("fixture-run");
        provider.relation_id = id("fixture-relation-provider", RelationId::new);
        provider.source_resource_id = source.resource_id.clone();
        provider.target_resource_id = target.resource_id.clone();
        provider.evidence_key = id("fixture-evidence-provider", EvidenceKey::new);

        let mut configured = provider.clone();
        configured.relation_id = id("fixture-relation-configured", RelationId::new);
        configured.evidence_key = id("fixture-evidence-configured", EvidenceKey::new);
        configured.evidence = RelationEvidence::Configured {
            binding_id: id("fixture-binding", BindingId::new),
        };

        let mut inferred = provider.clone();
        inferred.relation_id = id("fixture-relation-inferred", RelationId::new);
        inferred.evidence_key = id("fixture-evidence-inferred", EvidenceKey::new);
        inferred.evidence = RelationEvidence::Inferred {
            rule_version: id("fixture-rule-v1", RuleVersion::new),
            input_resource_version_ids: Vec::new(),
            input_relation_version_ids: vec![id(
                "fixture-input-relation-version",
                RelationVersionId::new,
            )],
            confidence: Confidence::from_basis_points(8_500).unwrap(),
        };

        store
            .commit_sync(SyncCommit {
                sync_run: run(SyncRunStatus::Succeeded, Some(timestamp_at(3))),
                resources: vec![source.clone(), target],
                resource_versions: Vec::new(),
                relations: vec![provider, configured, inferred],
                relation_versions: Vec::new(),
                changes: Vec::new(),
                cursor_after: Some(id("cursor-after", SyncCursor::new)),
                missing_evidence: None,
            })
            .unwrap();
        store
            .connection
            .execute(
                "INSERT INTO bindings(binding_id, source_resource_id, target_resource_id, kind, status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    "fixture-binding",
                    source.resource_id.as_str(),
                    "fixture-resource-target",
                    "fixture.depends_on",
                    "active",
                    17_i64,
                    18_i64,
                ],
            )
            .unwrap();

        let detail = store
            .query_resource_detail(&source.resource_id)
            .unwrap()
            .body
            .unwrap();
        assert_eq!(detail.relations.len(), 3);

        let provider = detail
            .relations
            .iter()
            .find(|projected| {
                matches!(
                    projected.relation.evidence,
                    RelationEvidence::Provider { .. }
                )
            })
            .unwrap();
        assert_eq!(
            provider.provider_connector_type,
            Some(ConnectorType::new("fixture").unwrap())
        );
        assert_eq!(provider.configured_created_at, None);

        let configured = detail
            .relations
            .iter()
            .find(|projected| {
                matches!(
                    projected.relation.evidence,
                    RelationEvidence::Configured { .. }
                )
            })
            .unwrap();
        assert_eq!(configured.provider_connector_type, None);
        assert_eq!(configured.configured_created_at, Some(timestamp_at(17)));

        let inferred = detail
            .relations
            .iter()
            .find(|projected| {
                matches!(
                    projected.relation.evidence,
                    RelationEvidence::Inferred { .. }
                )
            })
            .unwrap();
        assert_eq!(inferred.provider_connector_type, None);
        assert_eq!(inferred.configured_created_at, None);

        let frontier = store
            .query_relations_for_resources(&BTreeSet::from([source.resource_id]), 10, None)
            .unwrap();
        assert_eq!(frontier.body.items.len(), 3);
        assert_eq!(
            frontier
                .body
                .items
                .iter()
                .filter(|projected| projected.provider_connector_type.is_some())
                .count(),
            1
        );
    }

    #[test]
    fn projection_rejects_missing_freshness_context_and_invalid_bounds() {
        let (_directory, store) = query_projection_store();
        let base = ResourceProjectionPlan {
            query: None,
            kinds: BTreeSet::new(),
            connector_types: BTreeSet::new(),
            health: Vec::new(),
            freshness: Vec::new(),
            labels: BTreeMap::new(),
            cutoffs: projection_cutoffs(),
            limit: 10,
            after: None,
        };

        let mut missing_context = base.clone();
        missing_context.cutoffs.clear();
        assert!(store.query_resources(&missing_context).is_err());

        let mut invalid_limit = base;
        invalid_limit.limit = 0;
        assert!(store.query_resources(&invalid_limit).is_err());

        let invalid_cursor = RecentChangesProjectionPlan {
            since_millis: None,
            resource_id: None,
            kinds: BTreeSet::new(),
            limit: 10,
            after: Some("not-a-change-cursor".into()),
        };
        assert!(store.query_recent_changes(&invalid_cursor).is_err());
    }

    #[test]
    fn binding_commit_persists_provenance_and_advances_revision() {
        let (_directory, mut store, source, target) = topology_store();
        let binding = binding(&source, &target);
        let relation = configured_relation(&binding, Lifecycle::Active, 11);
        let relation_version = configured_relation_version(
            &relation,
            "fixture-configured-version",
            &binding.binding_id,
            11,
        );
        let change = binding_change(&binding, "fixture-binding-change", 11);
        let revision_before = store.projection_metadata().unwrap().committed_revision;

        store
            .commit_binding(BindingCommit {
                binding: binding.clone(),
                relations: vec![relation.clone()],
                relation_versions: vec![relation_version.clone()],
                changes: vec![change.clone()],
            })
            .unwrap();

        assert_eq!(
            store.projection_metadata().unwrap().committed_revision,
            revision_before + 1
        );
        assert_eq!(
            store.get_binding(&binding.binding_id).unwrap(),
            Some(binding)
        );
        assert_eq!(
            store.get_relation(&relation.relation_id).unwrap(),
            Some(relation)
        );
        assert!(
            store
                .relation_version_exists(&relation_version.relation_version_id)
                .unwrap()
        );

        let persisted_origin: String = store
            .connection
            .query_row(
                "SELECT origin_json FROM relation_versions WHERE relation_version_id = ?1",
                params![relation_version.relation_version_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<OriginRef>(&persisted_origin).unwrap(),
            relation_version.origin
        );

        let persisted_change = store
            .connection
            .query_row(
                "SELECT change_id, subject_type, subject_id, observed_at, fields_json, origin_json FROM changes WHERE change_id = ?1",
                params![change.change_id.as_str()],
                read_change,
            )
            .unwrap();
        assert_eq!(persisted_change, change);
    }

    #[test]
    fn failed_binding_commit_rolls_back_binding_relations_versions_changes_and_revision() {
        let (_directory, mut store, source, target) = topology_store();
        let binding = binding(&source, &target);
        let relation = configured_relation(&binding, Lifecycle::Active, 11);
        let valid_version = configured_relation_version(
            &relation,
            "fixture-configured-version-valid",
            &binding.binding_id,
            11,
        );
        let mut invalid_version = configured_relation_version(
            &relation,
            "fixture-configured-version-invalid",
            &binding.binding_id,
            12,
        );
        invalid_version.relation_id = id("missing-configured-relation", RelationId::new);
        let change = binding_change(&binding, "fixture-binding-change-failed", 11);
        let revision_before = store.projection_metadata().unwrap().committed_revision;

        assert!(
            store
                .commit_binding(BindingCommit {
                    binding: binding.clone(),
                    relations: vec![relation.clone()],
                    relation_versions: vec![valid_version, invalid_version],
                    changes: vec![change],
                })
                .is_err()
        );

        assert_eq!(
            store.projection_metadata().unwrap().committed_revision,
            revision_before
        );
        assert!(store.get_binding(&binding.binding_id).unwrap().is_none());
        assert!(store.get_relation(&relation.relation_id).unwrap().is_none());
        let counts: (i64, i64, i64, i64) = store
            .connection
            .query_row(
                "SELECT (SELECT count(*) FROM relation_versions), (SELECT count(*) FROM changes), (SELECT count(*) FROM bindings), (SELECT count(*) FROM relations)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(counts, (0, 0, 0, 0));
    }

    #[test]
    fn inference_commit_persists_outputs_provenance_and_advances_revision() {
        let (_directory, mut store, source, target) = topology_store();
        let rule_version = id("fixture-rule-v1", RuleVersion::new);
        let input_resource_version_ids =
            vec![id("fixture-input-resource-version", ResourceVersionId::new)];
        let input_relation_version_ids =
            vec![id("fixture-input-relation-version", RelationVersionId::new)];
        let relation = inferred_relation(
            "fixture-inferred-relation",
            &rule_version,
            &source,
            &target,
            Lifecycle::Active,
            20,
            (&input_resource_version_ids, &input_relation_version_ids),
        );
        let relation_version =
            inference_relation_version(&relation, "fixture-inferred-relation-version", 20);
        let change = inference_change(&relation, "fixture-inferred-change", 20);
        let run = InferenceRun {
            inference_run_id: id("fixture-inference-run", InferenceRunId::new),
            rule_version: rule_version.clone(),
            started_at: timestamp_at(19),
            finished_at: Some(timestamp_at(20)),
            status: InferenceRunStatus::Completed,
            input_resource_version_ids: input_resource_version_ids.clone(),
            input_relation_version_ids: input_relation_version_ids.clone(),
            output_relation_ids: vec![relation.relation_id.clone()],
        };
        let revision_before = store.projection_metadata().unwrap().committed_revision;

        store
            .commit_inference(InferenceCommit {
                run: run.clone(),
                relations: vec![relation.clone()],
                relation_versions: vec![relation_version.clone()],
                changes: vec![change.clone()],
            })
            .unwrap();

        assert_eq!(
            store.projection_metadata().unwrap().committed_revision,
            revision_before + 1
        );
        assert_eq!(
            store.get_relation(&relation.relation_id).unwrap(),
            Some(relation.clone())
        );
        assert!(
            store
                .relation_version_exists(&relation_version.relation_version_id)
                .unwrap()
        );

        let persisted_run: (String, i64, Option<i64>, String, String, String, String) = store
            .connection
            .query_row(
                "SELECT rule_version, started_at, finished_at, status, input_resource_version_ids_json, input_relation_version_ids_json, summary_json FROM inference_runs WHERE inference_run_id = ?1",
                params![run.inference_run_id.as_str()],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(persisted_run.0, rule_version.as_str());
        assert_eq!(persisted_run.1, run.started_at.unix_millis());
        assert_eq!(persisted_run.2, run.finished_at.map(Timestamp::unix_millis));
        assert_eq!(persisted_run.3, "completed");
        assert_eq!(
            serde_json::from_str::<Vec<ResourceVersionId>>(&persisted_run.4).unwrap(),
            input_resource_version_ids
        );
        assert_eq!(
            serde_json::from_str::<Vec<RelationVersionId>>(&persisted_run.5).unwrap(),
            input_relation_version_ids
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&persisted_run.6).unwrap(),
            json!({"output_relation_ids": ["fixture-inferred-relation"]})
        );

        let output: (String, String) = store
            .connection
            .query_row(
                "SELECT relation_id, evidence_key FROM inference_outputs WHERE inference_run_id = ?1",
                params![run.inference_run_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            output,
            (
                relation.relation_id.to_string(),
                relation.evidence_key.to_string()
            )
        );

        let persisted_evidence: String = store
            .connection
            .query_row(
                "SELECT evidence_json FROM relations WHERE relation_id = ?1",
                params![relation.relation_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<RelationEvidence>(&persisted_evidence).unwrap(),
            relation.evidence
        );

        let persisted_version_origin: String = store
            .connection
            .query_row(
                "SELECT origin_json FROM relation_versions WHERE relation_version_id = ?1",
                params![relation_version.relation_version_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<OriginRef>(&persisted_version_origin).unwrap(),
            relation_version.origin
        );
        let persisted_change = store
            .connection
            .query_row(
                "SELECT change_id, subject_type, subject_id, observed_at, fields_json, origin_json FROM changes WHERE change_id = ?1",
                params![change.change_id.as_str()],
                read_change,
            )
            .unwrap();
        assert_eq!(persisted_change, change);
    }

    #[test]
    fn failed_inference_commit_rolls_back_run_relations_versions_outputs_changes_and_revision() {
        let (_directory, mut store, source, target) = topology_store();
        let rule_version = id("fixture-rule-failed", RuleVersion::new);
        let relation = inferred_relation(
            "fixture-inferred-failed",
            &rule_version,
            &source,
            &target,
            Lifecycle::Active,
            30,
            (&[], &[]),
        );
        let relation_version =
            inference_relation_version(&relation, "fixture-inferred-failed-version", 30);
        let change = inference_change(&relation, "fixture-inferred-failed-change", 30);
        let run = InferenceRun {
            inference_run_id: id("fixture-inference-run-failed", InferenceRunId::new),
            rule_version,
            started_at: timestamp_at(29),
            finished_at: Some(timestamp_at(30)),
            status: InferenceRunStatus::Completed,
            input_resource_version_ids: Vec::new(),
            input_relation_version_ids: Vec::new(),
            output_relation_ids: vec![
                relation.relation_id.clone(),
                id("missing-inference-output", RelationId::new),
            ],
        };
        let revision_before = store.projection_metadata().unwrap().committed_revision;

        assert!(
            store
                .commit_inference(InferenceCommit {
                    run: run.clone(),
                    relations: vec![relation.clone()],
                    relation_versions: vec![relation_version],
                    changes: vec![change],
                })
                .is_err()
        );

        assert_eq!(
            store.projection_metadata().unwrap().committed_revision,
            revision_before
        );
        assert!(store.get_relation(&relation.relation_id).unwrap().is_none());
        let counts: (i64, i64, i64, i64, i64, i64) = store
            .connection
            .query_row(
                "SELECT (SELECT count(*) FROM inference_runs), (SELECT count(*) FROM relations), (SELECT count(*) FROM relation_versions), (SELECT count(*) FROM inference_outputs), (SELECT count(*) FROM changes), (SELECT count(*) FROM projection_metadata WHERE committed_revision = ?1)",
                params![revision_before as i64],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(counts, (0, 0, 0, 0, 0, 1));
    }

    #[test]
    fn inferred_relations_for_rule_filters_rule_and_reads_tombstoned_relations() {
        let (_directory, mut store, source, target) = topology_store();
        let input_resource_version_ids = vec![id(
            "fixture-filter-resource-version",
            ResourceVersionId::new,
        )];
        let input_relation_version_ids = vec![id(
            "fixture-filter-relation-version",
            RelationVersionId::new,
        )];
        let rule_v1 = id("fixture-filter-rule-v1", RuleVersion::new);
        let rule_v2 = id("fixture-filter-rule-v2", RuleVersion::new);
        let active_v1 = inferred_relation(
            "fixture-inferred-active-v1",
            &rule_v1,
            &source,
            &target,
            Lifecycle::Active,
            40,
            (&input_resource_version_ids, &input_relation_version_ids),
        );
        let stale_v1 = inferred_relation(
            "fixture-inferred-stale-v1",
            &rule_v1,
            &source,
            &target,
            Lifecycle::Tombstoned,
            40,
            (&input_resource_version_ids, &input_relation_version_ids),
        );
        let active_v2 = inferred_relation(
            "fixture-inferred-active-v2",
            &rule_v2,
            &source,
            &target,
            Lifecycle::Active,
            41,
            (&input_resource_version_ids, &input_relation_version_ids),
        );

        store
            .commit_inference(InferenceCommit {
                run: InferenceRun {
                    inference_run_id: id("fixture-inference-run-filter-v1", InferenceRunId::new),
                    rule_version: rule_v1.clone(),
                    started_at: timestamp_at(39),
                    finished_at: Some(timestamp_at(40)),
                    status: InferenceRunStatus::Completed,
                    input_resource_version_ids: input_resource_version_ids.clone(),
                    input_relation_version_ids: input_relation_version_ids.clone(),
                    output_relation_ids: vec![active_v1.relation_id.clone()],
                },
                relations: vec![active_v1.clone(), stale_v1.clone()],
                relation_versions: vec![
                    inference_relation_version(&active_v1, "fixture-filter-version-active-v1", 40),
                    inference_relation_version(&stale_v1, "fixture-filter-version-stale-v1", 40),
                ],
                changes: vec![
                    inference_change(&active_v1, "fixture-filter-change-active-v1", 40),
                    inference_change(&stale_v1, "fixture-filter-change-stale-v1", 40),
                ],
            })
            .unwrap();
        store
            .commit_inference(InferenceCommit {
                run: InferenceRun {
                    inference_run_id: id("fixture-inference-run-filter-v2", InferenceRunId::new),
                    rule_version: rule_v2.clone(),
                    started_at: timestamp_at(40),
                    finished_at: Some(timestamp_at(41)),
                    status: InferenceRunStatus::Completed,
                    input_resource_version_ids,
                    input_relation_version_ids,
                    output_relation_ids: vec![active_v2.relation_id.clone()],
                },
                relations: vec![active_v2.clone()],
                relation_versions: vec![inference_relation_version(
                    &active_v2,
                    "fixture-filter-version-active-v2",
                    41,
                )],
                changes: vec![inference_change(
                    &active_v2,
                    "fixture-filter-change-active-v2",
                    41,
                )],
            })
            .unwrap();

        let v1_relations = store.inferred_relations_for_rule(&rule_v1).unwrap();
        assert_eq!(v1_relations.len(), 2);
        assert!(
            v1_relations
                .iter()
                .all(|relation| relation.evidence.evidence_type() == EvidenceType::Inferred)
        );
        let stale = v1_relations
            .iter()
            .find(|relation| relation.relation_id == stale_v1.relation_id)
            .unwrap();
        assert_eq!(stale.lifecycle, Lifecycle::Tombstoned);
        assert!(
            v1_relations
                .iter()
                .all(|relation| relation.relation_id != active_v2.relation_id)
        );
        assert_eq!(
            store.get_relation(&stale_v1.relation_id).unwrap(),
            Some(stale_v1)
        );

        let v2_relations = store.inferred_relations_for_rule(&rule_v2).unwrap();
        assert_eq!(v2_relations, vec![active_v2]);
    }
}
