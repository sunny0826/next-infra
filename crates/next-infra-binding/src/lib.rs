//! Local configured-relation service. Binding mutations never modify provider facts.

use next_infra_core::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fmt::{self, Write};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BindingInput {
    pub source_resource_id: ResourceId,
    pub target_resource_id: ResourceId,
    pub kind: RelationKind,
}

#[derive(Debug)]
pub enum BindingError<E> {
    Store(E),
    Invalid(DomainError),
    NotFound,
    Conflict(&'static str),
}

impl<E: fmt::Display> fmt::Display for BindingError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "binding store failed: {error}"),
            Self::Invalid(error) => write!(formatter, "binding is invalid: {error}"),
            Self::NotFound => formatter.write_str("binding was not found"),
            Self::Conflict(message) => formatter.write_str(message),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for BindingError<E> {}

pub struct BindingService<'a, S> {
    store: &'a mut S,
}

impl<'a, S> BindingService<'a, S>
where
    S: BindingStore,
{
    pub fn new(store: &'a mut S) -> Self {
        Self { store }
    }

    pub fn create(
        &mut self,
        input: BindingInput,
        at: Timestamp,
    ) -> Result<Binding, BindingError<S::Error>> {
        validate_input(&input).map_err(BindingError::Invalid)?;
        let status = self.endpoint_status(&input)?;
        let binding = Binding {
            binding_id: BindingId::new(format!("binding:v1:{}", Uuid::new_v4()))
                .map_err(BindingError::Invalid)?,
            source_resource_id: input.source_resource_id,
            target_resource_id: input.target_resource_id,
            kind: input.kind,
            status,
            created_at: at,
            updated_at: at,
        };
        let commit = binding_commit(None, binding.clone(), at).map_err(BindingError::Invalid)?;
        self.store
            .commit_binding(commit)
            .map_err(BindingError::Store)?;
        Ok(binding)
    }

    pub fn update(
        &mut self,
        binding_id: &BindingId,
        input: BindingInput,
        at: Timestamp,
    ) -> Result<Binding, BindingError<S::Error>> {
        validate_input(&input).map_err(BindingError::Invalid)?;
        let existing = self.binding(binding_id)?;
        ensure_later(at, existing.updated_at)?;
        let status = if existing.status == BindingStatus::Disabled {
            BindingStatus::Disabled
        } else {
            self.endpoint_status(&input)?
        };
        let binding = Binding {
            binding_id: existing.binding_id.clone(),
            source_resource_id: input.source_resource_id,
            target_resource_id: input.target_resource_id,
            kind: input.kind,
            status,
            created_at: existing.created_at,
            updated_at: at,
        };
        if binding == existing {
            return Ok(existing);
        }
        let commit =
            binding_commit(Some(&existing), binding.clone(), at).map_err(BindingError::Invalid)?;
        self.store
            .commit_binding(commit)
            .map_err(BindingError::Store)?;
        Ok(binding)
    }

    pub fn disable(
        &mut self,
        binding_id: &BindingId,
        at: Timestamp,
    ) -> Result<Binding, BindingError<S::Error>> {
        let existing = self.binding(binding_id)?;
        if existing.status == BindingStatus::Disabled {
            return Ok(existing);
        }
        ensure_later(at, existing.updated_at)?;
        let mut binding = existing.clone();
        binding.status = BindingStatus::Disabled;
        binding.updated_at = at;
        let commit =
            binding_commit(Some(&existing), binding.clone(), at).map_err(BindingError::Invalid)?;
        self.store
            .commit_binding(commit)
            .map_err(BindingError::Store)?;
        Ok(binding)
    }

    pub fn reconcile(
        &mut self,
        binding_id: &BindingId,
        at: Timestamp,
    ) -> Result<Binding, BindingError<S::Error>> {
        let existing = self.binding(binding_id)?;
        if existing.status == BindingStatus::Disabled {
            return Ok(existing);
        }
        let input = BindingInput {
            source_resource_id: existing.source_resource_id.clone(),
            target_resource_id: existing.target_resource_id.clone(),
            kind: existing.kind.clone(),
        };
        let status = self.endpoint_status(&input)?;
        if status == existing.status {
            return Ok(existing);
        }
        ensure_later(at, existing.updated_at)?;
        let mut binding = existing.clone();
        binding.status = status;
        binding.updated_at = at;
        let commit =
            binding_commit(Some(&existing), binding.clone(), at).map_err(BindingError::Invalid)?;
        self.store
            .commit_binding(commit)
            .map_err(BindingError::Store)?;
        Ok(binding)
    }

    fn binding(&self, id: &BindingId) -> Result<Binding, BindingError<S::Error>> {
        self.store
            .get_binding(id)
            .map_err(BindingError::Store)?
            .ok_or(BindingError::NotFound)
    }

    fn endpoint_status(
        &self,
        input: &BindingInput,
    ) -> Result<BindingStatus, BindingError<S::Error>> {
        let source = self
            .store
            .get_resource(&input.source_resource_id)
            .map_err(BindingError::Store)?;
        let target = self
            .store
            .get_resource(&input.target_resource_id)
            .map_err(BindingError::Store)?;
        Ok(
            if source.is_some_and(|resource| resource.lifecycle == Lifecycle::Active)
                && target.is_some_and(|resource| resource.lifecycle == Lifecycle::Active)
            {
                BindingStatus::Active
            } else {
                BindingStatus::Unresolved
            },
        )
    }
}

fn validate_input(input: &BindingInput) -> Result<(), DomainError> {
    if input.source_resource_id == input.target_resource_id {
        return Err(DomainError::invalid_value(
            "binding source and target must be different",
        ));
    }
    Ok(())
}

fn ensure_later<E>(at: Timestamp, previous: Timestamp) -> Result<(), BindingError<E>> {
    if at <= previous {
        return Err(BindingError::Conflict("binding mutation time must advance"));
    }
    Ok(())
}

fn binding_commit(
    previous: Option<&Binding>,
    binding: Binding,
    at: Timestamp,
) -> Result<BindingCommit, DomainError> {
    let identity_changed = previous.is_some_and(|old| {
        old.source_resource_id != binding.source_resource_id
            || old.target_resource_id != binding.target_resource_id
            || old.kind != binding.kind
    });
    let mut relations = Vec::new();
    if let Some(old) = previous.filter(|_| identity_changed) {
        relations.push(configured_relation(old, Lifecycle::Tombstoned, at)?);
    }
    relations.push(configured_relation(
        &binding,
        relation_lifecycle(binding.status),
        at,
    )?);
    let relation_versions = relations
        .iter()
        .map(|relation| relation_version(relation, &binding.binding_id, at))
        .collect::<Result<Vec<_>, _>>()?;
    let fields = binding_changes(previous, &binding)?;
    let changes = if fields.is_empty() {
        Vec::new()
    } else {
        vec![Change {
            change_id: ChangeId::new(format!(
                "binding-change:{}:{}",
                binding.binding_id,
                at.unix_millis()
            ))?,
            subject: ChangeSubject::Binding {
                binding_id: binding.binding_id.clone(),
            },
            observed_at: at,
            fields,
            origin: OriginRef::Binding {
                binding_id: binding.binding_id.clone(),
            },
        }]
    };
    Ok(BindingCommit {
        binding,
        relations,
        relation_versions,
        changes,
    })
}

fn configured_relation(
    binding: &Binding,
    lifecycle: Lifecycle,
    at: Timestamp,
) -> Result<Relation, DomainError> {
    Ok(Relation {
        relation_id: relation_id(binding)?,
        source_resource_id: binding.source_resource_id.clone(),
        target_resource_id: binding.target_resource_id.clone(),
        kind: binding.kind.clone(),
        evidence_key: EvidenceKey::new(format!("binding:{}", binding.binding_id))?,
        evidence: RelationEvidence::Configured {
            binding_id: binding.binding_id.clone(),
        },
        first_seen_at: binding.created_at,
        last_seen_at: at,
        lifecycle,
    })
}

fn relation_id(binding: &Binding) -> Result<RelationId, DomainError> {
    RelationId::new(format!(
        "binding-relation:{}:{}:{}:{}",
        binding.binding_id, binding.source_resource_id, binding.target_resource_id, binding.kind
    ))
}

fn relation_lifecycle(status: BindingStatus) -> Lifecycle {
    match status {
        BindingStatus::Active => Lifecycle::Active,
        BindingStatus::Unresolved => Lifecycle::Orphaned,
        BindingStatus::Disabled => Lifecycle::Tombstoned,
    }
}

fn relation_version(
    relation: &Relation,
    binding_id: &BindingId,
    at: Timestamp,
) -> Result<RelationVersion, DomainError> {
    let snapshot = json!({
        "source_resource_id": relation.source_resource_id,
        "target_resource_id": relation.target_resource_id,
        "kind": relation.kind,
        "evidence": relation.evidence,
        "lifecycle": relation.lifecycle,
    });
    Ok(RelationVersion {
        relation_version_id: RelationVersionId::new(format!(
            "binding-relation-version:{}:{}",
            relation.relation_id,
            at.unix_millis()
        ))?,
        relation_id: relation.relation_id.clone(),
        observed_at: at,
        fingerprint: fingerprint(&snapshot)?,
        normalized_snapshot: snapshot,
        schema_version: SchemaVersion::new(1)?,
        origin: OriginRef::Binding {
            binding_id: binding_id.clone(),
        },
    })
}

fn binding_changes(
    previous: Option<&Binding>,
    current: &Binding,
) -> Result<Vec<FieldChange>, DomainError> {
    let mut changes = Vec::new();
    field_change(
        &mut changes,
        "binding.source_resource_id",
        previous.map(|value| json!(value.source_resource_id)),
        json!(current.source_resource_id),
    )?;
    field_change(
        &mut changes,
        "binding.target_resource_id",
        previous.map(|value| json!(value.target_resource_id)),
        json!(current.target_resource_id),
    )?;
    field_change(
        &mut changes,
        "binding.kind",
        previous.map(|value| json!(value.kind)),
        json!(current.kind),
    )?;
    field_change(
        &mut changes,
        "binding.status",
        previous.map(|value| json!(value.status)),
        json!(current.status),
    )?;
    Ok(changes)
}

fn field_change(
    changes: &mut Vec<FieldChange>,
    path: &str,
    before: Option<Value>,
    after: Value,
) -> Result<(), DomainError> {
    if before.as_ref() != Some(&after) {
        changes.push(FieldChange {
            path: FieldPath::new(path)?,
            before,
            after: Some(after),
        });
    }
    Ok(())
}

fn fingerprint(value: &Value) -> Result<Fingerprint, DomainError> {
    let encoded = serde_json::to_vec(value)
        .map_err(|_| DomainError::invalid_value("binding snapshot cannot be encoded"))?;
    let digest = Sha256::digest(encoded);
    let mut fingerprint = String::with_capacity(7 + digest.len() * 2);
    fingerprint.push_str("sha256:");
    for byte in digest {
        write!(&mut fingerprint, "{byte:02x}").expect("writing to String cannot fail");
    }
    Fingerprint::new(fingerprint)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};
    use std::convert::Infallible;

    #[derive(Default)]
    struct MemoryStore {
        resources: BTreeMap<ResourceId, Resource>,
        bindings: BTreeMap<BindingId, Binding>,
        commits: Vec<BindingCommit>,
    }

    impl StoreReader for MemoryStore {
        type Error = Infallible;

        fn get_connection(&self, _id: &ConnectionId) -> Result<Option<Connection>, Self::Error> {
            Ok(None)
        }

        fn get_resource(&self, id: &ResourceId) -> Result<Option<Resource>, Self::Error> {
            Ok(self.resources.get(id).cloned())
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
            Ok(self.resources.values().cloned().collect())
        }
    }

    impl BindingStore for MemoryStore {
        fn get_binding(&self, id: &BindingId) -> Result<Option<Binding>, Self::Error> {
            Ok(self.bindings.get(id).cloned())
        }

        fn list_bindings(&self) -> Result<Vec<Binding>, Self::Error> {
            Ok(self.bindings.values().cloned().collect())
        }

        fn commit_binding(&mut self, commit: BindingCommit) -> Result<(), Self::Error> {
            self.bindings
                .insert(commit.binding.binding_id.clone(), commit.binding.clone());
            self.commits.push(commit);
            Ok(())
        }
    }

    fn id<T>(value: &str, constructor: impl FnOnce(String) -> Result<T, DomainError>) -> T {
        constructor(value.to_owned()).unwrap()
    }

    fn resource(value: &str, lifecycle: Lifecycle) -> Resource {
        Resource {
            resource_id: id(value, ResourceId::new),
            connection_id: id("fixture-connection", ConnectionId::new),
            kind: id("fixture.resource", ResourceKind::new),
            external_id: id(format!("external-{value}").as_str(), ExternalId::new),
            name: value.into(),
            display_name: value.into(),
            scope: id("fixture-scope", Scope::new),
            labels: BTreeMap::new(),
            lifecycle,
            health: ResourceHealth::Unknown,
            attributes: json!({}),
            attribute_schema_version: SchemaVersion::new(1).unwrap(),
            fingerprint: id(format!("fingerprint-{value}").as_str(), Fingerprint::new),
            first_seen_at: Timestamp::from_unix_millis(1).unwrap(),
            last_seen_at: Timestamp::from_unix_millis(1).unwrap(),
            last_changed_at: Timestamp::from_unix_millis(1).unwrap(),
            last_sync_run_id: id("fixture-run", SyncRunId::new),
        }
    }

    fn input(source: &str, target: &str, kind: &str) -> BindingInput {
        BindingInput {
            source_resource_id: id(source, ResourceId::new),
            target_resource_id: id(target, ResourceId::new),
            kind: id(kind, RelationKind::new),
        }
    }

    fn store() -> MemoryStore {
        let mut store = MemoryStore::default();
        store.resources.insert(
            id("source", ResourceId::new),
            resource("source", Lifecycle::Active),
        );
        store.resources.insert(
            id("target", ResourceId::new),
            resource("target", Lifecycle::Active),
        );
        store.resources.insert(
            id("other", ResourceId::new),
            resource("other", Lifecycle::Active),
        );
        store
    }

    #[test]
    fn create_update_and_disable_preserve_configured_evidence_history() {
        let mut store = store();
        let binding = BindingService::new(&mut store)
            .create(
                input("source", "target", "fixture.depends_on"),
                Timestamp::from_unix_millis(10).unwrap(),
            )
            .unwrap();
        assert_eq!(binding.status, BindingStatus::Active);
        assert_eq!(store.commits[0].relations[0].lifecycle, Lifecycle::Active);
        assert!(matches!(
            store.commits[0].relations[0].evidence,
            RelationEvidence::Configured { .. }
        ));

        let updated = BindingService::new(&mut store)
            .update(
                &binding.binding_id,
                input("source", "other", "fixture.runs_on"),
                Timestamp::from_unix_millis(20).unwrap(),
            )
            .unwrap();
        assert_eq!(updated.target_resource_id.as_str(), "other");
        assert_eq!(store.commits[1].relations.len(), 2);
        assert_eq!(
            store.commits[1].relations[0].lifecycle,
            Lifecycle::Tombstoned
        );
        assert_eq!(store.commits[1].relations[1].lifecycle, Lifecycle::Active);

        let disabled = BindingService::new(&mut store)
            .disable(
                &binding.binding_id,
                Timestamp::from_unix_millis(30).unwrap(),
            )
            .unwrap();
        assert_eq!(disabled.status, BindingStatus::Disabled);
        assert_eq!(
            store.commits[2].relations[0].lifecycle,
            Lifecycle::Tombstoned
        );
        assert!(store.commits.iter().all(|commit| {
            commit.changes.iter().all(|change| {
                matches!(change.subject, ChangeSubject::Binding { .. })
                    && matches!(change.origin, OriginRef::Binding { .. })
            })
        }));
    }

    #[test]
    fn reconcile_marks_missing_endpoint_unresolved_and_recovers() {
        let mut store = store();
        let binding = BindingService::new(&mut store)
            .create(
                input("source", "target", "fixture.depends_on"),
                Timestamp::from_unix_millis(10).unwrap(),
            )
            .unwrap();
        store
            .resources
            .get_mut(&id("target", ResourceId::new))
            .unwrap()
            .lifecycle = Lifecycle::Tombstoned;
        let unresolved = BindingService::new(&mut store)
            .reconcile(
                &binding.binding_id,
                Timestamp::from_unix_millis(20).unwrap(),
            )
            .unwrap();
        assert_eq!(unresolved.status, BindingStatus::Unresolved);
        assert_eq!(store.commits[1].relations[0].lifecycle, Lifecycle::Orphaned);

        store
            .resources
            .get_mut(&id("target", ResourceId::new))
            .unwrap()
            .lifecycle = Lifecycle::Active;
        let recovered = BindingService::new(&mut store)
            .reconcile(
                &binding.binding_id,
                Timestamp::from_unix_millis(30).unwrap(),
            )
            .unwrap();
        assert_eq!(recovered.status, BindingStatus::Active);
        assert_eq!(store.commits[2].relations[0].lifecycle, Lifecycle::Active);
    }

    #[test]
    fn self_binding_is_rejected_without_commit() {
        let mut store = store();
        let error = BindingService::new(&mut store)
            .create(
                input("source", "source", "fixture.depends_on"),
                Timestamp::from_unix_millis(10).unwrap(),
            )
            .unwrap_err();
        assert!(matches!(error, BindingError::Invalid(_)));
        assert!(store.commits.is_empty());
    }

    #[test]
    fn relation_id_is_stable_for_the_same_binding_projection() {
        let mut store = store();
        let binding = BindingService::new(&mut store)
            .create(
                input("source", "target", "fixture.depends_on"),
                Timestamp::from_unix_millis(10).unwrap(),
            )
            .unwrap();
        let first_id = store.commits[0].relations[0].relation_id.clone();
        store
            .resources
            .get_mut(&id("target", ResourceId::new))
            .unwrap()
            .lifecycle = Lifecycle::Tombstoned;
        BindingService::new(&mut store)
            .reconcile(
                &binding.binding_id,
                Timestamp::from_unix_millis(20).unwrap(),
            )
            .unwrap();
        assert_eq!(store.commits[1].relations[0].relation_id, first_id);
        assert_eq!(
            store
                .commits
                .iter()
                .flat_map(|commit| &commit.relations)
                .map(|relation| relation.evidence_key.clone())
                .collect::<BTreeSet<_>>()
                .len(),
            1
        );
    }
}
