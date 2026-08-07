//! Deterministic inferred-relation materialization with explicit provenance.

use next_infra_core::*;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write};
use uuid::Uuid;

pub const MAX_INFERENCE_CANDIDATES: usize = 1_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct InferenceCandidate {
    pub source_resource_id: ResourceId,
    pub target_resource_id: ResourceId,
    pub kind: RelationKind,
    pub input_resource_version_ids: Vec<ResourceVersionId>,
    pub input_relation_version_ids: Vec<RelationVersionId>,
    pub confidence: Confidence,
}

#[derive(Debug)]
pub enum InferenceError<E> {
    Store(E),
    Invalid(DomainError),
    RuleNotRegistered,
    InputVersionMissing,
    CandidateLimit,
    ConflictingOutput,
}

impl<E: fmt::Display> fmt::Display for InferenceError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "inference store failed: {error}"),
            Self::Invalid(error) => write!(formatter, "inference is invalid: {error}"),
            Self::RuleNotRegistered => formatter.write_str("inference rule is not registered"),
            Self::InputVersionMissing => formatter.write_str("inference input version is missing"),
            Self::CandidateLimit => formatter.write_str("inference candidate limit was exceeded"),
            Self::ConflictingOutput => formatter.write_str("inference candidates conflict"),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for InferenceError<E> {}

pub struct InferenceEngine<'a, S> {
    store: &'a mut S,
    registered_rules: BTreeSet<RuleVersion>,
}

impl<'a, S> InferenceEngine<'a, S>
where
    S: InferenceStore,
{
    pub fn new(store: &'a mut S, registered_rules: impl IntoIterator<Item = RuleVersion>) -> Self {
        Self {
            store,
            registered_rules: registered_rules.into_iter().collect(),
        }
    }

    pub fn run(
        &mut self,
        rule_version: &RuleVersion,
        candidates: impl IntoIterator<Item = InferenceCandidate>,
        at: Timestamp,
    ) -> Result<InferenceRun, InferenceError<S::Error>> {
        if !self.registered_rules.contains(rule_version) {
            return Err(InferenceError::RuleNotRegistered);
        }
        let candidates = candidates.into_iter().collect::<Vec<_>>();
        if candidates.len() > MAX_INFERENCE_CANDIDATES {
            return Err(InferenceError::CandidateLimit);
        }
        let existing = self
            .store
            .inferred_relations_for_rule(rule_version)
            .map_err(InferenceError::Store)?;
        let existing_by_id = existing
            .iter()
            .map(|relation| (relation.relation_id.clone(), relation.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut produced = BTreeMap::new();
        let mut all_resource_inputs = BTreeSet::new();
        let mut all_relation_inputs = BTreeSet::new();

        for mut candidate in candidates {
            normalize_candidate(&mut candidate).map_err(InferenceError::Invalid)?;
            self.validate_candidate(&candidate)?;
            all_resource_inputs.extend(candidate.input_resource_version_ids.iter().cloned());
            all_relation_inputs.extend(candidate.input_relation_version_ids.iter().cloned());
            let relation_id =
                candidate_relation_id(rule_version, &candidate).map_err(InferenceError::Invalid)?;
            let relation = inferred_relation(
                rule_version,
                &candidate,
                at,
                existing_by_id.get(&relation_id),
            )
            .map_err(InferenceError::Invalid)?;
            match produced.get(&relation.relation_id) {
                Some(previous) if previous != &relation => {
                    return Err(InferenceError::ConflictingOutput);
                }
                Some(_) => {}
                None => {
                    produced.insert(relation.relation_id.clone(), relation);
                }
            }
        }

        let mut relations = produced.values().cloned().collect::<Vec<_>>();
        for old in existing {
            if !produced.contains_key(&old.relation_id) && old.lifecycle != Lifecycle::Tombstoned {
                let mut tombstoned = old;
                tombstoned.lifecycle = Lifecycle::Tombstoned;
                tombstoned.last_seen_at = at;
                relations.push(tombstoned);
            }
        }
        relations.sort_by_key(|relation| relation.relation_id.clone());

        let inference_run_id = InferenceRunId::new(format!("inference-run:v1:{}", Uuid::new_v4()))
            .map_err(InferenceError::Invalid)?;
        let output_relation_ids = produced.keys().cloned().collect::<Vec<_>>();
        let run = InferenceRun {
            inference_run_id,
            rule_version: rule_version.clone(),
            started_at: at,
            finished_at: Some(at),
            status: InferenceRunStatus::Completed,
            input_resource_version_ids: all_resource_inputs.into_iter().collect(),
            input_relation_version_ids: all_relation_inputs.into_iter().collect(),
            output_relation_ids,
        };
        let mut relation_versions = Vec::new();
        let mut changes = Vec::new();
        for relation in &relations {
            let previous = existing_by_id.get(&relation.relation_id);
            if previous == Some(relation) {
                continue;
            }
            relation_versions
                .push(relation_version(relation, at).map_err(InferenceError::Invalid)?);
            changes.push(relation_change(previous, relation, at).map_err(InferenceError::Invalid)?);
        }
        self.store
            .commit_inference(InferenceCommit {
                run: run.clone(),
                relations,
                relation_versions,
                changes,
            })
            .map_err(InferenceError::Store)?;
        Ok(run)
    }

    fn validate_candidate(
        &self,
        candidate: &InferenceCandidate,
    ) -> Result<(), InferenceError<S::Error>> {
        let source = self
            .store
            .get_resource(&candidate.source_resource_id)
            .map_err(InferenceError::Store)?;
        let target = self
            .store
            .get_resource(&candidate.target_resource_id)
            .map_err(InferenceError::Store)?;
        if !source.is_some_and(|resource| resource.lifecycle == Lifecycle::Active)
            || !target.is_some_and(|resource| resource.lifecycle == Lifecycle::Active)
        {
            return Err(InferenceError::InputVersionMissing);
        }
        for id in &candidate.input_resource_version_ids {
            if !self
                .store
                .resource_version_exists(id)
                .map_err(InferenceError::Store)?
            {
                return Err(InferenceError::InputVersionMissing);
            }
        }
        for id in &candidate.input_relation_version_ids {
            if !self
                .store
                .relation_version_exists(id)
                .map_err(InferenceError::Store)?
            {
                return Err(InferenceError::InputVersionMissing);
            }
        }
        Ok(())
    }
}

fn normalize_candidate(candidate: &mut InferenceCandidate) -> Result<(), DomainError> {
    if candidate.source_resource_id == candidate.target_resource_id {
        return Err(DomainError::invalid_value(
            "inference source and target must be different",
        ));
    }
    candidate.input_resource_version_ids.sort();
    candidate.input_resource_version_ids.dedup();
    candidate.input_relation_version_ids.sort();
    candidate.input_relation_version_ids.dedup();
    if candidate.input_resource_version_ids.is_empty()
        && candidate.input_relation_version_ids.is_empty()
    {
        return Err(DomainError::invalid_value(
            "inference requires versioned inputs",
        ));
    }
    Ok(())
}

fn inferred_relation(
    rule_version: &RuleVersion,
    candidate: &InferenceCandidate,
    at: Timestamp,
    existing: Option<&Relation>,
) -> Result<Relation, DomainError> {
    let relation_id = candidate_relation_id(rule_version, candidate)?;
    Ok(Relation {
        relation_id,
        source_resource_id: candidate.source_resource_id.clone(),
        target_resource_id: candidate.target_resource_id.clone(),
        kind: candidate.kind.clone(),
        evidence_key: candidate_evidence_key(rule_version, candidate)?,
        evidence: RelationEvidence::Inferred {
            rule_version: rule_version.clone(),
            input_resource_version_ids: candidate.input_resource_version_ids.clone(),
            input_relation_version_ids: candidate.input_relation_version_ids.clone(),
            confidence: candidate.confidence,
        },
        first_seen_at: existing.map_or(at, |relation| relation.first_seen_at),
        last_seen_at: at,
        lifecycle: Lifecycle::Active,
    })
}

fn candidate_relation_id(
    rule_version: &RuleVersion,
    candidate: &InferenceCandidate,
) -> Result<RelationId, DomainError> {
    RelationId::new(format!(
        "inferred-relation:{}",
        candidate_digest(rule_version, candidate)?
    ))
}

fn candidate_evidence_key(
    rule_version: &RuleVersion,
    candidate: &InferenceCandidate,
) -> Result<EvidenceKey, DomainError> {
    EvidenceKey::new(format!(
        "inference:{}",
        candidate_digest(rule_version, candidate)?
    ))
}

fn candidate_digest(
    rule_version: &RuleVersion,
    candidate: &InferenceCandidate,
) -> Result<String, DomainError> {
    digest(&json!({
        "rule_version": rule_version,
        "source_resource_id": candidate.source_resource_id,
        "target_resource_id": candidate.target_resource_id,
        "kind": candidate.kind,
        "input_resource_version_ids": candidate.input_resource_version_ids,
        "input_relation_version_ids": candidate.input_relation_version_ids,
    }))
}

fn relation_version(relation: &Relation, at: Timestamp) -> Result<RelationVersion, DomainError> {
    let snapshot = relation_snapshot(relation);
    let (rule_version, resource_inputs, relation_inputs) = inference_origin(&relation.evidence)?;
    Ok(RelationVersion {
        relation_version_id: RelationVersionId::new(format!(
            "inference-relation-version:{}:{}",
            relation.relation_id,
            at.unix_millis()
        ))?,
        relation_id: relation.relation_id.clone(),
        observed_at: at,
        normalized_snapshot: snapshot.clone(),
        fingerprint: Fingerprint::new(format!("sha256:{}", digest(&snapshot)?))?,
        schema_version: SchemaVersion::new(1)?,
        origin: OriginRef::Inference {
            rule_version,
            input_resource_version_ids: resource_inputs,
            input_relation_version_ids: relation_inputs,
        },
    })
}

fn relation_change(
    previous: Option<&Relation>,
    relation: &Relation,
    at: Timestamp,
) -> Result<Change, DomainError> {
    let (rule_version, resource_inputs, relation_inputs) = inference_origin(&relation.evidence)?;
    Ok(Change {
        change_id: ChangeId::new(format!(
            "inference-change:{}:{}",
            relation.relation_id,
            at.unix_millis()
        ))?,
        subject: ChangeSubject::Relation {
            relation_id: relation.relation_id.clone(),
        },
        observed_at: at,
        fields: vec![FieldChange {
            path: FieldPath::new("relation.snapshot")?,
            before: previous.map(relation_snapshot),
            after: Some(relation_snapshot(relation)),
        }],
        origin: OriginRef::Inference {
            rule_version,
            input_resource_version_ids: resource_inputs,
            input_relation_version_ids: relation_inputs,
        },
    })
}

fn inference_origin(
    evidence: &RelationEvidence,
) -> Result<(RuleVersion, Vec<ResourceVersionId>, Vec<RelationVersionId>), DomainError> {
    match evidence {
        RelationEvidence::Inferred {
            rule_version,
            input_resource_version_ids,
            input_relation_version_ids,
            ..
        } => Ok((
            rule_version.clone(),
            input_resource_version_ids.clone(),
            input_relation_version_ids.clone(),
        )),
        _ => Err(DomainError::invalid_value(
            "inference output has non-inferred evidence",
        )),
    }
}

fn relation_snapshot(relation: &Relation) -> Value {
    json!({
        "source_resource_id": relation.source_resource_id,
        "target_resource_id": relation.target_resource_id,
        "kind": relation.kind,
        "evidence": relation.evidence,
        "lifecycle": relation.lifecycle,
    })
}

fn digest(value: &Value) -> Result<String, DomainError> {
    let bytes = serde_json::to_vec(value)
        .map_err(|_| DomainError::invalid_value("inference value cannot be encoded"))?;
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut encoded, "{byte:02x}").expect("writing to String cannot fail");
    }
    Ok(encoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::Infallible;

    #[derive(Default)]
    struct MemoryStore {
        resources: BTreeMap<ResourceId, Resource>,
        resource_versions: BTreeSet<ResourceVersionId>,
        relation_versions: BTreeSet<RelationVersionId>,
        inferred: BTreeMap<RelationId, Relation>,
        commits: Vec<InferenceCommit>,
    }

    impl StoreReader for MemoryStore {
        type Error = Infallible;

        fn get_connection(&self, _id: &ConnectionId) -> Result<Option<Connection>, Self::Error> {
            Ok(None)
        }

        fn get_resource(&self, id: &ResourceId) -> Result<Option<Resource>, Self::Error> {
            Ok(self.resources.get(id).cloned())
        }

        fn get_relation(&self, id: &RelationId) -> Result<Option<Relation>, Self::Error> {
            Ok(self.inferred.get(id).cloned())
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

    impl InferenceStore for MemoryStore {
        fn resource_version_exists(&self, id: &ResourceVersionId) -> Result<bool, Self::Error> {
            Ok(self.resource_versions.contains(id))
        }

        fn relation_version_exists(&self, id: &RelationVersionId) -> Result<bool, Self::Error> {
            Ok(self.relation_versions.contains(id))
        }

        fn inferred_relations_for_rule(
            &self,
            rule_version: &RuleVersion,
        ) -> Result<Vec<Relation>, Self::Error> {
            Ok(self
                .inferred
                .values()
                .filter(|relation| {
                    matches!(
                        &relation.evidence,
                        RelationEvidence::Inferred { rule_version: value, .. }
                            if value == rule_version
                    )
                })
                .cloned()
                .collect())
        }

        fn commit_inference(&mut self, commit: InferenceCommit) -> Result<(), Self::Error> {
            for relation in &commit.relations {
                self.inferred
                    .insert(relation.relation_id.clone(), relation.clone());
            }
            self.commits.push(commit);
            Ok(())
        }
    }

    fn id<T>(value: &str, constructor: impl FnOnce(String) -> Result<T, DomainError>) -> T {
        constructor(value.to_owned()).unwrap()
    }

    fn resource(value: &str) -> Resource {
        Resource {
            resource_id: id(value, ResourceId::new),
            connection_id: id("fixture-connection", ConnectionId::new),
            kind: id("fixture.resource", ResourceKind::new),
            external_id: id(format!("external-{value}").as_str(), ExternalId::new),
            name: value.into(),
            display_name: value.into(),
            scope: id("fixture-scope", Scope::new),
            labels: BTreeMap::new(),
            lifecycle: Lifecycle::Active,
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

    fn store() -> MemoryStore {
        let mut store = MemoryStore::default();
        for value in ["source", "target", "other"] {
            store
                .resources
                .insert(id(value, ResourceId::new), resource(value));
        }
        store.resource_versions.extend([
            id("rv-a", ResourceVersionId::new),
            id("rv-b", ResourceVersionId::new),
        ]);
        store
            .relation_versions
            .insert(id("relv-a", RelationVersionId::new));
        store
    }

    fn rule() -> RuleVersion {
        id("fixture-rule-v1", RuleVersion::new)
    }

    fn candidate(target: &str, reversed_inputs: bool) -> InferenceCandidate {
        let mut resource_inputs = vec![
            id("rv-a", ResourceVersionId::new),
            id("rv-b", ResourceVersionId::new),
        ];
        if reversed_inputs {
            resource_inputs.reverse();
        }
        InferenceCandidate {
            source_resource_id: id("source", ResourceId::new),
            target_resource_id: id(target, ResourceId::new),
            kind: id("fixture.depends_on", RelationKind::new),
            input_resource_version_ids: resource_inputs,
            input_relation_version_ids: vec![id("relv-a", RelationVersionId::new)],
            confidence: Confidence::from_basis_points(8_500).unwrap(),
        }
    }

    #[test]
    fn identical_inputs_are_deterministic_across_order_and_runs() {
        let mut first_store = store();
        let first = InferenceEngine::new(&mut first_store, [rule()])
            .run(
                &rule(),
                [candidate("target", false), candidate("other", false)],
                Timestamp::from_unix_millis(10).unwrap(),
            )
            .unwrap();
        let mut second_store = store();
        let second = InferenceEngine::new(&mut second_store, [rule()])
            .run(
                &rule(),
                [candidate("other", true), candidate("target", true)],
                Timestamp::from_unix_millis(20).unwrap(),
            )
            .unwrap();
        assert_eq!(first.output_relation_ids, second.output_relation_ids);
        assert_eq!(
            first_store.commits[0]
                .relations
                .iter()
                .map(|relation| relation.evidence_key.clone())
                .collect::<Vec<_>>(),
            second_store.commits[0]
                .relations
                .iter()
                .map(|relation| relation.evidence_key.clone())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn missing_or_unregistered_inputs_fail_without_commit() {
        let mut store = store();
        let error = InferenceEngine::new(&mut store, [])
            .run(
                &rule(),
                [candidate("target", false)],
                Timestamp::from_unix_millis(10).unwrap(),
            )
            .unwrap_err();
        assert!(matches!(error, InferenceError::RuleNotRegistered));

        let mut missing = candidate("target", false);
        missing.input_resource_version_ids = vec![id("missing", ResourceVersionId::new)];
        let error = InferenceEngine::new(&mut store, [rule()])
            .run(&rule(), [missing], Timestamp::from_unix_millis(10).unwrap())
            .unwrap_err();
        assert!(matches!(error, InferenceError::InputVersionMissing));
        assert!(store.commits.is_empty());
    }

    #[test]
    fn absent_recomputed_output_is_tombstoned_with_full_origin() {
        let mut store = store();
        InferenceEngine::new(&mut store, [rule()])
            .run(
                &rule(),
                [candidate("target", false)],
                Timestamp::from_unix_millis(10).unwrap(),
            )
            .unwrap();
        InferenceEngine::new(&mut store, [rule()])
            .run(&rule(), [], Timestamp::from_unix_millis(20).unwrap())
            .unwrap();
        let commit = &store.commits[1];
        assert_eq!(commit.relations[0].lifecycle, Lifecycle::Tombstoned);
        assert!(matches!(
            &commit.changes[0].origin,
            OriginRef::Inference {
                input_resource_version_ids,
                input_relation_version_ids,
                ..
            } if input_resource_version_ids.len() == 2 && input_relation_version_ids.len() == 1
        ));
    }
}
