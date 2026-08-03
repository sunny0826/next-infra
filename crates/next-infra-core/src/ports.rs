use crate::*;
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncCommit {
    pub sync_run: SyncRun,
    pub resources: Vec<Resource>,
    pub resource_versions: Vec<ResourceVersion>,
    pub relations: Vec<Relation>,
    pub relation_versions: Vec<RelationVersion>,
    pub changes: Vec<Change>,
    pub cursor_after: Option<SyncCursor>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommitResult {
    pub resources_written: usize,
    pub resource_versions_written: usize,
    pub relations_written: usize,
    pub relation_versions_written: usize,
    pub changes_written: usize,
}

pub trait StoreReader {
    type Error;

    fn get_connection(&self, id: &ConnectionId) -> Result<Option<Connection>, Self::Error>;
    fn get_resource(&self, id: &ResourceId) -> Result<Option<Resource>, Self::Error>;
    fn get_relation(&self, id: &RelationId) -> Result<Option<Relation>, Self::Error>;
    fn get_sync_run(&self, id: &SyncRunId) -> Result<Option<SyncRun>, Self::Error>;
    fn sync_cursor(&self, connection_id: &ConnectionId) -> Result<Option<SyncCursor>, Self::Error>;
    fn list_resources_for_scope(
        &self,
        connection_id: &ConnectionId,
        scope: &Scope,
    ) -> Result<Vec<Resource>, Self::Error>;
}

pub trait StoreWriter {
    type Error;

    fn upsert_connection(&mut self, connection: Connection) -> Result<(), Self::Error>;
    fn start_sync_run(&mut self, sync_run: SyncRun) -> Result<(), Self::Error>;
    fn commit_sync(&mut self, commit: SyncCommit) -> Result<CommitResult, Self::Error>;
    fn mark_running_syncs_interrupted(&mut self, at: Timestamp) -> Result<usize, Self::Error>;
}

pub trait ConnectorPort {
    type Error;

    fn connector_type(&self) -> &ConnectorType;
    fn validate_connection(&self, connection: &Connection) -> Result<(), Self::Error>;
}

pub trait SecretProvider {
    type Error;

    fn resolve(&self, secret_ref: &SecretRef) -> Result<SecretValue, Self::Error>;
}

pub struct SecretValue(Vec<u8>);

impl SecretValue {
    pub fn new(value: impl Into<Vec<u8>>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretValue([REDACTED])")
    }
}

impl Drop for SecretValue {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}
