use crate::WriterQueue;
use next_infra_connector_api::ResourceLocator;
use next_infra_core::{
    Change, ChangeId, ChangeSubject, CommitResult, Connection, DomainError, FieldChange, FieldPath,
    Lifecycle, MissingEvidenceState, OriginRef, Relation, RelationId, RelationVersion, Resource,
    ResourceId, ResourceKey, ResourceVersion, Scope, StoreReader, StoreWriter, SyncCommit,
    SyncCoverage, SyncCursor, SyncMode, SyncRun, SyncRunCounts, SyncRunId, SyncRunStatus,
    SyncRunWarning, SyncTrigger, Timestamp,
};
use next_infra_normalizer::{ValidatedBatch, ValidatedRelation, ValidatedResource};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::fmt;

const INITIAL_CURSOR: &str = "initial";

/// A running SyncRun plus the scope used to evaluate authoritative absence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncRunHandle {
    pub run: SyncRun,
    pub scope: Scope,
}

impl SyncRunHandle {
    pub fn sync_run_id(&self) -> &SyncRunId {
        &self.run.sync_run_id
    }
}

/// Inputs required to persist the start of one synchronization run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncRunStart {
    pub sync_run_id: SyncRunId,
    pub mode: SyncMode,
    pub trigger: SyncTrigger,
    pub scope: Scope,
    pub started_at: Timestamp,
    pub targeted_resources: Vec<ResourceLocator>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SyncEngineError<E> {
    Store(E),
    Domain(DomainError),
    InvalidRun(String),
    MissingResource(ResourceId),
}

impl<E> From<DomainError> for SyncEngineError<E> {
    fn from(error: DomainError) -> Self {
        Self::Domain(error)
    }
}

impl<E: fmt::Display> fmt::Display for SyncEngineError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "sync store error: {error}"),
            Self::Domain(error) => write!(formatter, "sync domain error: {error}"),
            Self::InvalidRun(message) => write!(formatter, "invalid sync run: {message}"),
            Self::MissingResource(id) => write!(formatter, "missing relation endpoint: {id}"),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for SyncEngineError<E> {}

/// Coordinates run lifecycle and delegates every completed batch to the one
/// FIFO writer queue. The connector and normalizer are intentionally absent:
/// callers hand this type an already validated `ValidatedBatch`.
pub struct SyncEngine<S> {
    writer: WriterQueue<S>,
}

impl<S> SyncEngine<S> {
    pub fn new(store: S) -> Self {
        Self {
            writer: WriterQueue::new(store),
        }
    }

    pub fn writer(&self) -> &WriterQueue<S> {
        &self.writer
    }

    pub fn writer_mut(&mut self) -> &mut WriterQueue<S> {
        &mut self.writer
    }

    pub fn into_store(self) -> S {
        self.writer.into_store()
    }
}

impl<S, E> SyncEngine<S>
where
    S: StoreReader<Error = E> + StoreWriter<Error = E>,
{
    /// Start one run and persist its running marker before a connector call.
    pub fn start(
        &mut self,
        connection: &Connection,
        start: SyncRunStart,
    ) -> Result<SyncRunHandle, SyncEngineError<E>> {
        let cursor_before = self
            .writer
            .store()
            .sync_cursor(&connection.connection_id)
            .map_err(SyncEngineError::Store)?;
        let coverage = initial_coverage(
            start.mode,
            &connection.connection_id,
            &start.scope,
            cursor_before.as_ref(),
            &start.targeted_resources,
        )?;
        let run = SyncRun {
            sync_run_id: start.sync_run_id,
            connection_id: connection.connection_id.clone(),
            mode: start.mode,
            trigger: start.trigger,
            started_at: start.started_at,
            finished_at: None,
            status: SyncRunStatus::Running,
            coverage,
            cursor_before,
            cursor_after: None,
            counts: SyncRunCounts::default(),
            errors: Vec::new(),
            warnings: Vec::new(),
        };
        self.writer
            .store_mut()
            .start_sync_run(run.clone())
            .map_err(SyncEngineError::Store)?;
        Ok(SyncRunHandle {
            run,
            scope: start.scope,
        })
    }

    /// Convert a normalized batch into one atomic SyncCommit.
    pub fn commit(
        &mut self,
        handle: SyncRunHandle,
        batch: ValidatedBatch,
        finished_at: Timestamp,
    ) -> Result<CommitResult, SyncEngineError<E>> {
        validate_handle(&handle, &batch.connection_id, &batch.sync_run_id)?;
        validate_coverage(&handle, &batch.coverage)?;
        let commit = self.build_commit(&handle, &batch, finished_at)?;
        self.writer.submit(commit).map_err(SyncEngineError::Store)
    }

    /// Finish a run after a connector/normalizer failure without advancing its
    /// cursor or missing-evidence counters.
    pub fn fail(
        &mut self,
        handle: SyncRunHandle,
        error: DomainError,
        finished_at: Timestamp,
    ) -> Result<CommitResult, SyncEngineError<E>> {
        if handle.run.status != SyncRunStatus::Running || handle.run.finished_at.is_some() {
            return Err(SyncEngineError::InvalidRun(
                "failed run must still be running".into(),
            ));
        }
        let mut run = handle.run;
        run.status = SyncRunStatus::Failed;
        run.finished_at = Some(finished_at);
        run.errors = vec![error];
        run.warnings = Vec::new();
        // Keep the last committed cursor visible to the Store writer. A
        // failed run must never clear or advance connector progress.
        run.cursor_after = run.cursor_before.clone();
        let cursor_after = run.cursor_after.clone();
        self.writer
            .submit(SyncCommit {
                sync_run: run,
                resources: Vec::new(),
                resource_versions: Vec::new(),
                relations: Vec::new(),
                relation_versions: Vec::new(),
                changes: Vec::new(),
                cursor_after,
                missing_evidence: None,
            })
            .map_err(SyncEngineError::Store)
    }

    /// Mark runs left in `running` state as interrupted during startup.
    pub fn recover(&mut self, at: Timestamp) -> Result<usize, SyncEngineError<E>> {
        self.writer
            .store_mut()
            .mark_running_syncs_interrupted(at)
            .map_err(SyncEngineError::Store)
    }

    fn build_commit(
        &self,
        handle: &SyncRunHandle,
        batch: &ValidatedBatch,
        finished_at: Timestamp,
    ) -> Result<SyncCommit, SyncEngineError<E>> {
        let mut run = handle.run.clone();
        run.status = if matches!(batch.coverage, SyncCoverage::Partial { .. }) {
            SyncRunStatus::Partial
        } else {
            SyncRunStatus::Succeeded
        };
        run.finished_at = Some(finished_at);
        run.coverage = batch.coverage.clone();
        run.cursor_after = batch.next_cursor.clone();

        let mut resources = Vec::new();
        let mut resource_versions = Vec::new();
        let mut relations = Vec::new();
        let mut relation_versions = Vec::new();
        let mut changes = Vec::new();
        let mut counts = SyncRunCounts {
            read: (batch.resources.len() + batch.relations.len()) as u64,
            warnings: batch.warnings.len() as u64,
            ..SyncRunCounts::default()
        };
        let mut observed_resource_ids = BTreeSet::new();

        for validated in &batch.resources {
            let resource_id = stable_resource_id(&validated.key)?;
            observed_resource_ids.insert(resource_id.clone());
            let existing = self
                .writer
                .store()
                .get_resource(&resource_id)
                .map_err(SyncEngineError::Store)?;
            let resource =
                project_resource(validated, resource_id.clone(), &run, existing.as_ref());
            let fingerprint_changed = existing
                .as_ref()
                .is_none_or(|previous| previous.fingerprint != resource.fingerprint);
            let lifecycle_changed = existing
                .as_ref()
                .is_some_and(|previous| previous.lifecycle != resource.lifecycle);
            if existing.is_none() {
                counts.created += 1;
            } else if fingerprint_changed || lifecycle_changed {
                counts.updated += 1;
            } else {
                counts.unchanged += 1;
            }
            if existing.is_none() || fingerprint_changed {
                let snapshot = resource_snapshot(&resource);
                let summary = resource_diff(existing.as_ref(), &resource)?;
                resource_versions.push(ResourceVersion {
                    version_id: resource_version_id(&resource.resource_id, &run.sync_run_id)?,
                    resource_id: resource.resource_id.clone(),
                    observed_at: validated.observed_at,
                    sync_run_id: run.sync_run_id.clone(),
                    normalized_snapshot: snapshot,
                    fingerprint: resource.fingerprint.clone(),
                    schema_version: resource.attribute_schema_version,
                    change_summary: summary.clone(),
                });
                changes.push(resource_change(
                    &resource,
                    existing.as_ref(),
                    summary,
                    &run.sync_run_id,
                    validated.observed_at,
                )?);
            } else if lifecycle_changed {
                let summary = resource_diff(existing.as_ref(), &resource)?;
                changes.push(resource_change(
                    &resource,
                    existing.as_ref(),
                    summary,
                    &run.sync_run_id,
                    validated.observed_at,
                )?);
            }
            resources.push(resource);
        }

        for validated in &batch.relations {
            let relation_id = stable_relation_id(&validated.key)?;
            let source_id = stable_resource_id(&validated.key.source)?;
            let target_id = stable_resource_id(&validated.key.target)?;
            ensure_relation_endpoint(self.writer.store(), &observed_resource_ids, &source_id)?;
            ensure_relation_endpoint(self.writer.store(), &observed_resource_ids, &target_id)?;
            let existing = self
                .writer
                .store()
                .get_relation(&relation_id)
                .map_err(SyncEngineError::Store)?;
            let latest_fingerprint = self
                .writer
                .store()
                .latest_relation_version_fingerprint(&relation_id)
                .map_err(SyncEngineError::Store)?;
            let relation = project_relation(
                validated,
                relation_id.clone(),
                source_id,
                target_id,
                &run,
                existing.as_ref(),
            );
            let fingerprint_changed = existing.as_ref().is_none_or(|_| {
                latest_fingerprint
                    .as_ref()
                    .is_none_or(|previous| previous != &validated.fingerprint)
            });
            let lifecycle_changed = existing
                .as_ref()
                .is_some_and(|previous| previous.lifecycle != relation.lifecycle);
            if existing.is_none() {
                counts.created += 1;
            } else if fingerprint_changed || lifecycle_changed {
                counts.updated += 1;
            } else {
                counts.unchanged += 1;
            }
            if existing.is_none() || fingerprint_changed {
                let summary = relation_diff(existing.as_ref(), &relation)?;
                relation_versions.push(RelationVersion {
                    relation_version_id: relation_version_id(
                        &relation.relation_id,
                        &run.sync_run_id,
                    )?,
                    relation_id: relation.relation_id.clone(),
                    observed_at: validated.observed_at,
                    normalized_snapshot: relation_snapshot(&relation),
                    fingerprint: validated.fingerprint.clone(),
                    schema_version: next_schema_version()?,
                    origin: OriginRef::SyncRun {
                        sync_run_id: run.sync_run_id.clone(),
                    },
                });
                changes.push(relation_change(
                    &relation,
                    existing.as_ref(),
                    summary,
                    &run.sync_run_id,
                    validated.observed_at,
                )?);
            } else if lifecycle_changed {
                let summary = relation_diff(existing.as_ref(), &relation)?;
                changes.push(relation_change(
                    &relation,
                    existing.as_ref(),
                    summary,
                    &run.sync_run_id,
                    validated.observed_at,
                )?);
            }
            relations.push(relation);
        }

        let missing_evidence = if let SyncCoverage::AuthoritativeFull { scope } = &batch.coverage {
            let mut state = self
                .writer
                .store()
                .missing_evidence_state(&run.connection_id, scope)
                .map_err(SyncEngineError::Store)?
                .unwrap_or_else(|| MissingEvidenceState::new(scope.clone()));
            for resource_id in &observed_resource_ids {
                state.counts.remove(resource_id);
            }
            let existing_resources = self
                .writer
                .store()
                .list_resources_for_scope(&run.connection_id, scope)
                .map_err(SyncEngineError::Store)?;
            for existing in existing_resources {
                if observed_resource_ids.contains(&existing.resource_id) {
                    continue;
                }
                let count = state.count_for(&existing.resource_id).saturating_add(1);
                state
                    .counts
                    .insert(existing.resource_id.clone(), count.min(2));
                if count >= 2 && existing.lifecycle != Lifecycle::Tombstoned {
                    let tombstoned = tombstone_resource(&existing, &run, finished_at);
                    let summary = resource_diff(Some(&existing), &tombstoned)?;
                    changes.push(resource_change(
                        &tombstoned,
                        Some(&existing),
                        summary,
                        &run.sync_run_id,
                        finished_at,
                    )?);
                    resources.push(tombstoned);
                    counts.updated += 1;
                }
            }
            Some(state)
        } else {
            None
        };

        run.counts = counts;
        run.warnings = batch
            .warnings
            .iter()
            .map(|w| SyncRunWarning {
                code: w.code,
                message: w.message.clone(),
            })
            .collect();
        Ok(SyncCommit {
            sync_run: run,
            resources,
            resource_versions,
            relations,
            relation_versions,
            changes,
            cursor_after: batch.next_cursor.clone(),
            missing_evidence,
        })
    }
}

impl<S> WriterQueue<S> {
    fn into_store(self) -> S {
        self.into_inner()
    }
}

fn validate_handle<E>(
    handle: &SyncRunHandle,
    connection_id: &next_infra_core::ConnectionId,
    sync_run_id: &SyncRunId,
) -> Result<(), SyncEngineError<E>> {
    if handle.run.status != SyncRunStatus::Running || handle.run.finished_at.is_some() {
        return Err(SyncEngineError::InvalidRun(
            "commit requires a running SyncRun".into(),
        ));
    }
    if &handle.run.connection_id != connection_id || &handle.run.sync_run_id != sync_run_id {
        return Err(SyncEngineError::InvalidRun(
            "validated batch provenance does not match SyncRun".into(),
        ));
    }
    Ok(())
}

fn validate_coverage<E>(
    handle: &SyncRunHandle,
    coverage: &SyncCoverage,
) -> Result<(), SyncEngineError<E>> {
    let compatible = matches!(
        (handle.run.mode, coverage),
        (SyncMode::Full, SyncCoverage::AuthoritativeFull { .. })
            | (SyncMode::Full, SyncCoverage::Partial { .. })
            | (SyncMode::Incremental, SyncCoverage::Incremental { .. })
            | (SyncMode::Incremental, SyncCoverage::Partial { .. })
            | (SyncMode::Targeted, SyncCoverage::Targeted { .. })
            | (SyncMode::Targeted, SyncCoverage::Partial { .. })
    );
    if !compatible {
        return Err(SyncEngineError::Domain(DomainError::invalid_value(
            "validated coverage is incompatible with SyncRun mode",
        )));
    }
    match coverage {
        SyncCoverage::AuthoritativeFull { scope }
        | SyncCoverage::Partial {
            scope: Some(scope), ..
        } if scope != &handle.scope => Err(SyncEngineError::Domain(DomainError::invalid_value(
            "validated coverage scope does not match SyncRun scope",
        ))),
        _ => Ok(()),
    }
}

fn initial_coverage<E>(
    mode: SyncMode,
    connection_id: &next_infra_core::ConnectionId,
    scope: &Scope,
    cursor_before: Option<&SyncCursor>,
    targeted_resources: &[ResourceLocator],
) -> Result<SyncCoverage, SyncEngineError<E>> {
    match mode {
        SyncMode::Full => Ok(SyncCoverage::AuthoritativeFull {
            scope: scope.clone(),
        }),
        SyncMode::Incremental => Ok(SyncCoverage::Incremental {
            cursor: cursor_before
                .cloned()
                .unwrap_or(SyncCursor::new(INITIAL_CURSOR).map_err(SyncEngineError::Domain)?),
        }),
        SyncMode::Targeted => Ok(SyncCoverage::Targeted {
            resource_ids: targeted_resources
                .iter()
                .map(|resource| stable_resource_id_from_locator(connection_id, resource))
                .collect::<Result<Vec<_>, _>>()?,
        }),
    }
}

fn stable_resource_id(key: &ResourceKey) -> Result<ResourceId, DomainError> {
    ResourceId::new(format!(
        "resource:{}:{}:{}",
        key.connection_id.as_str(),
        key.kind.as_str(),
        key.external_id.as_str()
    ))
}

fn stable_resource_id_from_locator(
    connection_id: &next_infra_core::ConnectionId,
    locator: &ResourceLocator,
) -> Result<ResourceId, DomainError> {
    stable_resource_id(&ResourceKey {
        connection_id: connection_id.clone(),
        kind: locator.kind.clone(),
        external_id: locator.external_id.clone(),
    })
}

fn stable_relation_id(
    key: &next_infra_normalizer::ValidatedRelationKey,
) -> Result<RelationId, DomainError> {
    let source = stable_resource_id(&key.source)?;
    let target = stable_resource_id(&key.target)?;
    RelationId::new(format!(
        "relation:{}:{}:{}:{}",
        source.as_str(),
        target.as_str(),
        key.kind.as_str(),
        key.evidence_key.as_str()
    ))
}

fn project_resource(
    validated: &ValidatedResource,
    resource_id: ResourceId,
    run: &SyncRun,
    existing: Option<&Resource>,
) -> Resource {
    let mut resource = Resource {
        resource_id,
        connection_id: validated.key.connection_id.clone(),
        kind: validated.key.kind.clone(),
        external_id: validated.key.external_id.clone(),
        name: validated.name.clone(),
        display_name: validated.display_name.clone(),
        scope: validated.scope.clone(),
        labels: validated.labels.clone(),
        lifecycle: Lifecycle::Active,
        health: validated.health,
        attributes: validated.attributes.clone(),
        attribute_schema_version: validated.attribute_schema_version,
        fingerprint: validated.fingerprint.clone(),
        first_seen_at: existing.map_or(validated.observed_at, |old| old.first_seen_at),
        last_seen_at: validated.observed_at,
        last_changed_at: existing.map_or(validated.observed_at, |old| old.last_changed_at),
        last_sync_run_id: run.sync_run_id.clone(),
    };
    if existing.is_none_or(|old| {
        old.fingerprint != resource.fingerprint || old.lifecycle != Lifecycle::Active
    }) {
        resource.last_changed_at = validated.observed_at;
    }
    resource
}

fn project_relation(
    validated: &ValidatedRelation,
    relation_id: RelationId,
    source_resource_id: ResourceId,
    target_resource_id: ResourceId,
    _run: &SyncRun,
    existing: Option<&Relation>,
) -> Relation {
    Relation {
        relation_id,
        source_resource_id,
        target_resource_id,
        kind: validated.key.kind.clone(),
        evidence_key: validated.key.evidence_key.clone(),
        evidence: validated.evidence.clone(),
        first_seen_at: existing.map_or(validated.observed_at, |old| old.first_seen_at),
        last_seen_at: validated.observed_at,
        lifecycle: Lifecycle::Active,
    }
}

fn tombstone_resource(existing: &Resource, run: &SyncRun, at: Timestamp) -> Resource {
    let mut resource = existing.clone();
    resource.lifecycle = Lifecycle::Tombstoned;
    resource.last_changed_at = at;
    resource.last_sync_run_id = run.sync_run_id.clone();
    resource
}

fn resource_snapshot(resource: &Resource) -> Value {
    json!({
        "resource_id": resource.resource_id,
        "connection_id": resource.connection_id,
        "kind": resource.kind,
        "external_id": resource.external_id,
        "name": resource.name,
        "display_name": resource.display_name,
        "scope": resource.scope,
        "labels": resource.labels,
        "lifecycle": resource.lifecycle,
        "health": resource.health,
        "attributes": resource.attributes,
        "attribute_schema_version": resource.attribute_schema_version,
        "fingerprint": resource.fingerprint,
    })
}

fn relation_snapshot(relation: &Relation) -> Value {
    json!({
        "relation_id": relation.relation_id,
        "source_resource_id": relation.source_resource_id,
        "target_resource_id": relation.target_resource_id,
        "kind": relation.kind,
        "evidence_key": relation.evidence_key,
        "evidence": relation.evidence,
        "lifecycle": relation.lifecycle,
    })
}

fn resource_diff(
    existing: Option<&Resource>,
    resource: &Resource,
) -> Result<Vec<FieldChange>, DomainError> {
    let Some(existing) = existing else {
        return Ok(vec![FieldChange {
            path: FieldPath::new("resource")?,
            before: None,
            after: Some(resource_snapshot(resource)),
        }]);
    };
    let before = resource_snapshot(existing);
    let after = resource_snapshot(resource);
    diff_fields(
        &before,
        &after,
        [
            "name",
            "display_name",
            "scope",
            "labels",
            "lifecycle",
            "health",
            "attributes",
            "attribute_schema_version",
            "fingerprint",
        ],
    )
}

fn relation_diff(
    existing: Option<&Relation>,
    relation: &Relation,
) -> Result<Vec<FieldChange>, DomainError> {
    let Some(existing) = existing else {
        return Ok(vec![FieldChange {
            path: FieldPath::new("relation")?,
            before: None,
            after: Some(relation_snapshot(relation)),
        }]);
    };
    let before = relation_snapshot(existing);
    let after = relation_snapshot(relation);
    diff_fields(&before, &after, ["evidence", "lifecycle"])
}

fn diff_fields<const N: usize>(
    before: &Value,
    after: &Value,
    fields: [&str; N],
) -> Result<Vec<FieldChange>, DomainError> {
    fields
        .into_iter()
        .filter_map(|field| {
            let old = before.get(field).cloned();
            let new = after.get(field).cloned();
            (old != new).then_some((field, old, new))
        })
        .map(|(field, before, after)| {
            Ok(FieldChange {
                path: FieldPath::new(field)?,
                before,
                after,
            })
        })
        .collect()
}

fn resource_change(
    resource: &Resource,
    existing: Option<&Resource>,
    fields: Vec<FieldChange>,
    sync_run_id: &SyncRunId,
    observed_at: Timestamp,
) -> Result<Change, DomainError> {
    Ok(Change {
        change_id: ChangeId::new(format!(
            "change:{}:{}",
            resource.resource_id,
            sync_run_id.as_str()
        ))?,
        subject: ChangeSubject::Resource {
            resource_id: resource.resource_id.clone(),
        },
        observed_at,
        fields: if fields.is_empty() && existing.is_some() {
            vec![FieldChange {
                path: FieldPath::new("lifecycle")?,
                before: Some(json!(existing.map(|old| old.lifecycle))),
                after: Some(json!(resource.lifecycle)),
            }]
        } else {
            fields
        },
        origin: OriginRef::SyncRun {
            sync_run_id: sync_run_id.clone(),
        },
    })
}

fn relation_change(
    relation: &Relation,
    existing: Option<&Relation>,
    fields: Vec<FieldChange>,
    sync_run_id: &SyncRunId,
    observed_at: Timestamp,
) -> Result<Change, DomainError> {
    Ok(Change {
        change_id: ChangeId::new(format!(
            "change:{}:{}",
            relation.relation_id,
            sync_run_id.as_str()
        ))?,
        subject: ChangeSubject::Relation {
            relation_id: relation.relation_id.clone(),
        },
        observed_at,
        fields: if fields.is_empty() && existing.is_some() {
            vec![FieldChange {
                path: FieldPath::new("lifecycle")?,
                before: Some(json!(existing.map(|old| old.lifecycle))),
                after: Some(json!(relation.lifecycle)),
            }]
        } else {
            fields
        },
        origin: OriginRef::SyncRun {
            sync_run_id: sync_run_id.clone(),
        },
    })
}

fn resource_version_id(
    resource_id: &ResourceId,
    sync_run_id: &SyncRunId,
) -> Result<next_infra_core::ResourceVersionId, DomainError> {
    next_infra_core::ResourceVersionId::new(format!(
        "resource-version:{}:{}",
        resource_id, sync_run_id
    ))
}

fn relation_version_id(
    relation_id: &RelationId,
    sync_run_id: &SyncRunId,
) -> Result<next_infra_core::RelationVersionId, DomainError> {
    next_infra_core::RelationVersionId::new(format!(
        "relation-version:{}:{}",
        relation_id, sync_run_id
    ))
}

fn next_schema_version() -> Result<next_infra_core::SchemaVersion, DomainError> {
    next_infra_core::SchemaVersion::new(1)
}

fn ensure_relation_endpoint<S, E>(
    store: &S,
    planned_resource_ids: &BTreeSet<ResourceId>,
    id: &ResourceId,
) -> Result<(), SyncEngineError<E>>
where
    S: StoreReader<Error = E>,
{
    if planned_resource_ids.contains(id) {
        return Ok(());
    }
    if store
        .get_resource(id)
        .map_err(SyncEngineError::Store)?
        .is_none()
    {
        return Err(SyncEngineError::MissingResource(id.clone()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use next_infra_connector_api::{
        ConnectionInput, ObservationWarning, ProviderRequestSummary, RedactionReport,
        ResourceLocator, SyncRequest,
    };
    use next_infra_core::{
        ConnectorHealth, ConnectorType, ExternalId, Fingerprint, LabelKey, RelationEvidence,
        RelationKind, ResourceHealth, ResourceKind, SchemaVersion,
    };
    use serde_json::json;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct FakeStore {
        connection: Option<Connection>,
        resources: BTreeMap<String, Resource>,
        relations: BTreeMap<String, Relation>,
        relation_fingerprints: BTreeMap<String, Fingerprint>,
        runs: BTreeMap<String, SyncRun>,
        cursor: Option<SyncCursor>,
        missing: Option<MissingEvidenceState>,
        commits: Vec<SyncCommit>,
    }

    impl StoreReader for FakeStore {
        type Error = String;

        fn get_connection(
            &self,
            id: &next_infra_core::ConnectionId,
        ) -> Result<Option<Connection>, Self::Error> {
            Ok(self
                .connection
                .as_ref()
                .filter(|connection| &connection.connection_id == id)
                .cloned())
        }

        fn get_resource(&self, id: &ResourceId) -> Result<Option<Resource>, Self::Error> {
            Ok(self.resources.get(id.as_str()).cloned())
        }

        fn get_relation(&self, id: &RelationId) -> Result<Option<Relation>, Self::Error> {
            Ok(self.relations.get(id.as_str()).cloned())
        }

        fn latest_relation_version_fingerprint(
            &self,
            id: &RelationId,
        ) -> Result<Option<Fingerprint>, Self::Error> {
            Ok(self.relation_fingerprints.get(id.as_str()).cloned())
        }

        fn get_sync_run(&self, id: &SyncRunId) -> Result<Option<SyncRun>, Self::Error> {
            Ok(self.runs.get(id.as_str()).cloned())
        }

        fn sync_cursor(
            &self,
            _connection_id: &next_infra_core::ConnectionId,
        ) -> Result<Option<SyncCursor>, Self::Error> {
            Ok(self.cursor.clone())
        }

        fn list_resources_for_scope(
            &self,
            _connection_id: &next_infra_core::ConnectionId,
            scope: &Scope,
        ) -> Result<Vec<Resource>, Self::Error> {
            Ok(self
                .resources
                .values()
                .filter(|resource| &resource.scope == scope)
                .cloned()
                .collect())
        }

        fn missing_evidence_state(
            &self,
            _connection_id: &next_infra_core::ConnectionId,
            _scope: &Scope,
        ) -> Result<Option<MissingEvidenceState>, Self::Error> {
            Ok(self.missing.clone())
        }
    }

    impl StoreWriter for FakeStore {
        type Error = String;

        fn upsert_connection(&mut self, connection: Connection) -> Result<(), Self::Error> {
            self.connection = Some(connection);
            Ok(())
        }

        fn start_sync_run(&mut self, sync_run: SyncRun) -> Result<(), Self::Error> {
            self.runs
                .insert(sync_run.sync_run_id.as_str().to_owned(), sync_run);
            Ok(())
        }

        fn commit_sync(&mut self, commit: SyncCommit) -> Result<CommitResult, Self::Error> {
            self.cursor = commit.cursor_after.clone();
            self.runs.insert(
                commit.sync_run.sync_run_id.as_str().to_owned(),
                commit.sync_run.clone(),
            );
            for resource in &commit.resources {
                self.resources
                    .insert(resource.resource_id.as_str().to_owned(), resource.clone());
            }
            for relation in &commit.relations {
                self.relations
                    .insert(relation.relation_id.as_str().to_owned(), relation.clone());
            }
            for version in &commit.relation_versions {
                self.relation_fingerprints.insert(
                    version.relation_id.as_str().to_owned(),
                    version.fingerprint.clone(),
                );
            }
            self.missing = commit.missing_evidence.clone();
            self.commits.push(commit);
            Ok(CommitResult::default())
        }

        fn mark_running_syncs_interrupted(&mut self, at: Timestamp) -> Result<usize, Self::Error> {
            let mut count = 0;
            for run in self
                .runs
                .values_mut()
                .filter(|run| run.status == SyncRunStatus::Running)
            {
                run.status = SyncRunStatus::Interrupted;
                run.finished_at = Some(at);
                count += 1;
            }
            Ok(count)
        }
    }

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

    fn request(run_id: &str, mode: SyncMode, scope: &str, cursor: Option<&str>) -> SyncRequest {
        SyncRequest {
            sync_run_id: SyncRunId::new(run_id).unwrap(),
            connection: ConnectionInput {
                connection_id: next_infra_core::ConnectionId::new("fixture-connection").unwrap(),
                connector_type: ConnectorType::new("fixture").unwrap(),
                config: json!({}),
                config_schema_version: SchemaVersion::new(1).unwrap(),
            },
            mode,
            scope: Scope::new(scope).unwrap(),
            cursor: cursor.map(|value| SyncCursor::new(value).unwrap()),
            targeted_resources: Vec::new(),
        }
    }

    fn start_input(
        request: &SyncRequest,
        trigger: SyncTrigger,
        scope: &Scope,
        started_at: i64,
    ) -> SyncRunStart {
        SyncRunStart {
            sync_run_id: request.sync_run_id.clone(),
            mode: request.mode,
            trigger,
            scope: scope.clone(),
            started_at: Timestamp::from_unix_millis(started_at).unwrap(),
            targeted_resources: request.targeted_resources.clone(),
        }
    }

    fn batch(
        request: &SyncRequest,
        state: &str,
        coverage: SyncCoverage,
        cursor: Option<&str>,
    ) -> ValidatedBatch {
        let kind = ResourceKind::new("fixture.resource").unwrap();
        let external_id = ExternalId::new("fixture-resource-a").unwrap();
        let key = ResourceKey {
            connection_id: request.connection.connection_id.clone(),
            kind: kind.clone(),
            external_id: external_id.clone(),
        };
        ValidatedBatch {
            connection_id: request.connection.connection_id.clone(),
            sync_run_id: request.sync_run_id.clone(),
            resources: vec![ValidatedResource {
                key,
                name: "fixture-a".into(),
                display_name: "Fixture A".into(),
                scope: request.scope.clone(),
                labels: BTreeMap::<LabelKey, String>::new(),
                health: ResourceHealth::Healthy,
                attributes: json!({"state": state}),
                attribute_schema_version: SchemaVersion::new(1).unwrap(),
                observed_at: Timestamp::from_unix_millis(10).unwrap(),
                fingerprint: Fingerprint::new(format!("fingerprint-{state}")).unwrap(),
            }],
            relations: Vec::new(),
            coverage,
            next_cursor: cursor.map(|value| SyncCursor::new(value).unwrap()),
            warnings: Vec::<ObservationWarning>::new(),
            redaction_report: RedactionReport::default(),
            provider_request_summary: ProviderRequestSummary::default(),
        }
    }

    fn batch_with_relation(
        request: &SyncRequest,
        relation_fingerprint: &str,
        coverage: SyncCoverage,
        cursor: Option<&str>,
    ) -> ValidatedBatch {
        let mut batch = batch(request, "ready", coverage, cursor);
        let key = batch.resources[0].key.clone();
        batch.relations.push(ValidatedRelation {
            key: next_infra_normalizer::ValidatedRelationKey {
                source: key.clone(),
                target: key,
                kind: RelationKind::new("fixture.depends_on").unwrap(),
                evidence_key: next_infra_core::EvidenceKey::new("fixture-evidence").unwrap(),
            },
            evidence: RelationEvidence::Provider {
                connection_id: request.connection.connection_id.clone(),
                sync_run_id: request.sync_run_id.clone(),
                field_path: FieldPath::new("attributes.target").unwrap(),
            },
            observed_at: Timestamp::from_unix_millis(10).unwrap(),
            fingerprint: Fingerprint::new(relation_fingerprint).unwrap(),
        });
        batch
    }

    fn engine() -> SyncEngine<FakeStore> {
        SyncEngine::new(FakeStore::default())
    }

    #[test]
    fn same_fingerprint_does_not_create_a_second_version() {
        let mut engine = engine();
        let connection = connection();
        let scope = Scope::new("fixture-scope").unwrap();
        let request_one = request("fixture-run-1", SyncMode::Full, scope.as_str(), None);
        let first = engine
            .start(
                &connection,
                start_input(&request_one, SyncTrigger::User, &scope, 1),
            )
            .unwrap();
        engine
            .commit(
                first,
                batch(
                    &request_one,
                    "ready",
                    SyncCoverage::AuthoritativeFull {
                        scope: scope.clone(),
                    },
                    Some("cursor-1"),
                ),
                Timestamp::from_unix_millis(11).unwrap(),
            )
            .unwrap();

        let request_two = request(
            "fixture-run-2",
            SyncMode::Full,
            scope.as_str(),
            Some("cursor-1"),
        );
        let second = engine
            .start(
                &connection,
                start_input(&request_two, SyncTrigger::Schedule, &scope, 12),
            )
            .unwrap();
        engine
            .commit(
                second,
                batch(
                    &request_two,
                    "ready",
                    SyncCoverage::AuthoritativeFull { scope },
                    Some("cursor-2"),
                ),
                Timestamp::from_unix_millis(22).unwrap(),
            )
            .unwrap();

        let store = engine.into_store();
        assert_eq!(store.commits[0].resource_versions.len(), 1);
        assert!(store.commits[1].resource_versions.is_empty());
        assert_eq!(store.commits[1].sync_run.counts.unchanged, 1);
    }

    #[test]
    fn relation_versions_follow_latest_persisted_fingerprint() {
        let mut engine = engine();
        let connection = connection();
        let scope = Scope::new("fixture-scope").unwrap();
        for (run_id, fingerprint, cursor, started_at, finished_at) in [
            (
                "fixture-relation-run-1",
                "relation-fingerprint-v1",
                "cursor-1",
                1,
                2,
            ),
            (
                "fixture-relation-run-2",
                "relation-fingerprint-v1",
                "cursor-2",
                3,
                4,
            ),
            (
                "fixture-relation-run-3",
                "relation-fingerprint-v2",
                "cursor-3",
                5,
                6,
            ),
        ] {
            let request = request(run_id, SyncMode::Full, scope.as_str(), None);
            let handle = engine
                .start(
                    &connection,
                    start_input(&request, SyncTrigger::Schedule, &scope, started_at),
                )
                .unwrap();
            engine
                .commit(
                    handle,
                    batch_with_relation(
                        &request,
                        fingerprint,
                        SyncCoverage::AuthoritativeFull {
                            scope: scope.clone(),
                        },
                        Some(cursor),
                    ),
                    Timestamp::from_unix_millis(finished_at).unwrap(),
                )
                .unwrap();
        }

        let store = engine.into_store();
        assert_eq!(store.commits[0].relation_versions.len(), 1);
        assert!(store.commits[1].relation_versions.is_empty());
        assert_eq!(store.commits[2].relation_versions.len(), 1);
    }

    #[test]
    fn partial_and_incremental_do_not_add_missing_evidence() {
        let mut engine = engine();
        let connection = connection();
        let scope = Scope::new("fixture-scope").unwrap();
        let first_request = request("fixture-run-1", SyncMode::Full, scope.as_str(), None);
        let first = engine
            .start(
                &connection,
                start_input(&first_request, SyncTrigger::User, &scope, 1),
            )
            .unwrap();
        engine
            .commit(
                first,
                batch(
                    &first_request,
                    "ready",
                    SyncCoverage::AuthoritativeFull {
                        scope: scope.clone(),
                    },
                    Some("cursor-1"),
                ),
                Timestamp::from_unix_millis(11).unwrap(),
            )
            .unwrap();

        let partial_request = request(
            "fixture-run-2",
            SyncMode::Full,
            scope.as_str(),
            Some("cursor-1"),
        );
        let partial = engine
            .start(
                &connection,
                start_input(&partial_request, SyncTrigger::Schedule, &scope, 12),
            )
            .unwrap();
        engine
            .commit(
                partial,
                batch(
                    &partial_request,
                    "ready",
                    SyncCoverage::Partial {
                        scope: Some(scope.clone()),
                        reason: next_infra_core::CoverageGapReason::RateLimited,
                    },
                    Some("cursor-recover"),
                ),
                Timestamp::from_unix_millis(22).unwrap(),
            )
            .unwrap();

        let store = engine.into_store();
        assert!(store.commits[1].missing_evidence.is_none());
        assert_eq!(store.commits[1].sync_run.status, SyncRunStatus::Partial);
    }

    #[test]
    fn failed_run_does_not_advance_cursor_or_missing_evidence() {
        let store = FakeStore {
            cursor: Some(SyncCursor::new("cursor-before").unwrap()),
            ..FakeStore::default()
        };
        let mut engine = SyncEngine::new(store);
        let connection = connection();
        let scope = Scope::new("fixture-scope").unwrap();
        let req = request(
            "fixture-run-failed",
            SyncMode::Full,
            scope.as_str(),
            Some("cursor-before"),
        );
        let handle = engine
            .start(
                &connection,
                start_input(&req, SyncTrigger::Recovery, &scope, 1),
            )
            .unwrap();

        engine
            .fail(
                handle,
                DomainError {
                    code: next_infra_core::ErrorCode::ProviderUnavailable,
                    message: "fixture provider unavailable".into(),
                    retryable: true,
                },
                Timestamp::from_unix_millis(2).unwrap(),
            )
            .unwrap();

        let store = engine.into_store();
        assert_eq!(
            store.cursor.as_ref().map(SyncCursor::as_str),
            Some("cursor-before")
        );
        assert!(store.missing.is_none());
        assert_eq!(store.commits[0].sync_run.status, SyncRunStatus::Failed);
        assert_eq!(
            store.commits[0]
                .sync_run
                .cursor_after
                .as_ref()
                .map(SyncCursor::as_str),
            Some("cursor-before")
        );
    }

    #[test]
    fn incremental_and_targeted_runs_never_create_missing_evidence_state() {
        let store = FakeStore {
            cursor: Some(SyncCursor::new("cursor-v1").unwrap()),
            ..FakeStore::default()
        };
        let mut engine = SyncEngine::new(store);
        let connection = connection();
        let scope = Scope::new("fixture-scope").unwrap();

        let incremental_request = request(
            "fixture-run-incremental",
            SyncMode::Incremental,
            scope.as_str(),
            Some("cursor-v1"),
        );
        let incremental_handle = engine
            .start(
                &connection,
                start_input(&incremental_request, SyncTrigger::Schedule, &scope, 1),
            )
            .unwrap();
        engine
            .commit(
                incremental_handle,
                batch(
                    &incremental_request,
                    "changed",
                    SyncCoverage::Incremental {
                        cursor: SyncCursor::new("cursor-v1").unwrap(),
                    },
                    Some("cursor-v2"),
                ),
                Timestamp::from_unix_millis(2).unwrap(),
            )
            .unwrap();

        let targeted_request = request(
            "fixture-run-targeted",
            SyncMode::Targeted,
            scope.as_str(),
            Some("cursor-v2"),
        );
        let targeted_handle = engine
            .start(
                &connection,
                SyncRunStart {
                    sync_run_id: targeted_request.sync_run_id.clone(),
                    mode: targeted_request.mode,
                    trigger: SyncTrigger::User,
                    scope: scope.clone(),
                    started_at: Timestamp::from_unix_millis(3).unwrap(),
                    targeted_resources: vec![ResourceLocator {
                        kind: ResourceKind::new("fixture.resource").unwrap(),
                        external_id: ExternalId::new("fixture-resource-a").unwrap(),
                    }],
                },
            )
            .unwrap();
        engine
            .commit(
                targeted_handle,
                ValidatedBatch {
                    resources: Vec::new(),
                    relations: Vec::new(),
                    coverage: SyncCoverage::Targeted {
                        resource_ids: vec![
                            stable_resource_id(&ResourceKey {
                                connection_id: connection.connection_id.clone(),
                                kind: ResourceKind::new("fixture.resource").unwrap(),
                                external_id: ExternalId::new("fixture-resource-a").unwrap(),
                            })
                            .unwrap(),
                        ],
                    },
                    ..batch(
                        &targeted_request,
                        "unused",
                        SyncCoverage::Targeted {
                            resource_ids: Vec::new(),
                        },
                        None,
                    )
                },
                Timestamp::from_unix_millis(4).unwrap(),
            )
            .unwrap();

        let store = engine.into_store();
        assert!(store.missing.is_none());
        assert_eq!(store.commits[0].sync_run.status, SyncRunStatus::Succeeded);
        assert_eq!(store.commits[1].sync_run.status, SyncRunStatus::Succeeded);
    }

    #[test]
    fn two_authoritative_absences_tombstone_and_reappearance_restores_active() {
        let mut store = FakeStore::default();
        let connection = connection();
        store.connection = Some(connection.clone());
        let scope = Scope::new("fixture-scope").unwrap();
        let resource_id =
            ResourceId::new("resource:fixture-connection:fixture.resource:fixture-resource-a")
                .unwrap();
        store.resources.insert(
            resource_id.as_str().into(),
            Resource {
                resource_id: resource_id.clone(),
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
                last_sync_run_id: SyncRunId::new("fixture-old-run").unwrap(),
            },
        );
        let mut engine = SyncEngine::new(store);

        for (run_id, started, finished) in [("fixture-run-1", 2, 3), ("fixture-run-2", 4, 5)] {
            let req = request(run_id, SyncMode::Full, scope.as_str(), None);
            let handle = engine
                .start(
                    &connection,
                    start_input(&req, SyncTrigger::Schedule, &scope, started),
                )
                .unwrap();
            let empty = ValidatedBatch {
                resources: Vec::new(),
                relations: Vec::new(),
                ..batch(
                    &req,
                    "unused",
                    SyncCoverage::AuthoritativeFull {
                        scope: scope.clone(),
                    },
                    None,
                )
            };
            engine
                .commit(
                    handle,
                    empty,
                    Timestamp::from_unix_millis(finished).unwrap(),
                )
                .unwrap();
        }

        let store = engine.into_store();
        assert_eq!(
            store.resources[resource_id.as_str()].lifecycle,
            Lifecycle::Tombstoned
        );
        assert_eq!(store.missing.as_ref().unwrap().count_for(&resource_id), 2);

        let req = request("fixture-run-3", SyncMode::Full, scope.as_str(), None);
        let mut engine = SyncEngine::new(store);
        let handle = engine
            .start(
                &connection,
                start_input(&req, SyncTrigger::Recovery, &scope, 6),
            )
            .unwrap();
        engine
            .commit(
                handle,
                batch(
                    &req,
                    "ready",
                    SyncCoverage::AuthoritativeFull {
                        scope: scope.clone(),
                    },
                    None,
                ),
                Timestamp::from_unix_millis(7).unwrap(),
            )
            .unwrap();
        let store = engine.into_store();
        assert_eq!(
            store.resources[resource_id.as_str()].lifecycle,
            Lifecycle::Active
        );
        assert_eq!(store.missing.as_ref().unwrap().count_for(&resource_id), 0);
    }

    #[test]
    fn startup_recovery_marks_running_runs_interrupted() {
        let mut store = FakeStore::default();
        let run_id = SyncRunId::new("fixture-running").unwrap();
        store.runs.insert(
            run_id.as_str().into(),
            SyncRun {
                sync_run_id: run_id,
                connection_id: next_infra_core::ConnectionId::new("fixture-connection").unwrap(),
                mode: SyncMode::Full,
                trigger: SyncTrigger::Startup,
                started_at: Timestamp::from_unix_millis(1).unwrap(),
                finished_at: None,
                status: SyncRunStatus::Running,
                coverage: SyncCoverage::AuthoritativeFull {
                    scope: Scope::new("fixture-scope").unwrap(),
                },
                cursor_before: None,
                cursor_after: None,
                counts: SyncRunCounts::default(),
                errors: Vec::new(),
                warnings: Vec::new(),
            },
        );
        let mut engine = SyncEngine::new(store);

        assert_eq!(
            engine
                .recover(Timestamp::from_unix_millis(9).unwrap())
                .unwrap(),
            1
        );
        let store = engine.into_store();
        assert_eq!(
            store.runs["fixture-running"].status,
            SyncRunStatus::Interrupted
        );
    }

    #[test]
    fn relation_with_missing_endpoint_fails_before_queueing_commit() {
        let mut engine = engine();
        let connection = connection();
        let scope = Scope::new("fixture-scope").unwrap();
        let req = request("fixture-run", SyncMode::Full, scope.as_str(), None);
        let handle = engine
            .start(&connection, start_input(&req, SyncTrigger::User, &scope, 1))
            .unwrap();
        let mut invalid = batch(
            &req,
            "ready",
            SyncCoverage::AuthoritativeFull { scope },
            None,
        );
        invalid.relations.push(ValidatedRelation {
            key: next_infra_normalizer::ValidatedRelationKey {
                source: invalid.resources[0].key.clone(),
                target: ResourceKey {
                    connection_id: req.connection.connection_id.clone(),
                    kind: ResourceKind::new("fixture.resource").unwrap(),
                    external_id: ExternalId::new("missing-target").unwrap(),
                },
                kind: RelationKind::new("fixture.depends_on").unwrap(),
                evidence_key: next_infra_core::EvidenceKey::new("fixture-evidence").unwrap(),
            },
            evidence: RelationEvidence::Provider {
                connection_id: req.connection.connection_id.clone(),
                sync_run_id: req.sync_run_id.clone(),
                field_path: FieldPath::new("attributes.target").unwrap(),
            },
            observed_at: Timestamp::from_unix_millis(10).unwrap(),
            fingerprint: Fingerprint::new("relation-fingerprint").unwrap(),
        });

        assert!(matches!(
            engine.commit(handle, invalid, Timestamp::from_unix_millis(11).unwrap()),
            Err(SyncEngineError::MissingResource(_))
        ));
        assert_eq!(engine.writer().pending_len(), 0);
    }
}
