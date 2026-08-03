use crate::{ResourceId, Scope};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Persisted evidence that resources in one scope were absent from an
/// authoritative observation.
///
/// The state intentionally carries no connection identifier. A store scopes
/// reads and writes by `(ConnectionId, Scope)`, while a `SyncCommit` already
/// carries its connection through `SyncRun::connection_id`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissingEvidenceState {
    pub scope: Scope,
    #[serde(default)]
    pub counts: BTreeMap<ResourceId, u8>,
}

impl MissingEvidenceState {
    pub fn new(scope: Scope) -> Self {
        Self {
            scope,
            counts: BTreeMap::new(),
        }
    }

    pub fn with_counts(scope: Scope, counts: BTreeMap<ResourceId, u8>) -> Self {
        Self { scope, counts }
    }

    pub fn count_for(&self, resource_id: &ResourceId) -> u8 {
        self.counts.get(resource_id).copied().unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.counts.is_empty()
    }
}
