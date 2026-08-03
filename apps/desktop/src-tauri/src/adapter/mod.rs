//! Thin Desktop transport adapter.
//!
//! This module translates transport-shaped requests into the shared Query
//! Service. It owns no SQL, provider logic, or derived UI state. Tauri command
//! registration and event emission remain in the composition layer.

use next_infra_query::dto::{
    ChangePageDto, ConnectionSnapshotDto, ConnectorCoverageSnapshotDto, ErrorEnvelope, Freshness,
    HealthSummaryDto, ResourceDetailDto, ResourceHealth, ResourcePageDto, SyncStatusDto,
    TopologyDto,
};
use next_infra_query::service::{
    GetResourceRequest, GetTopologyRequest, QueryService, QuerySource, RecentChangesRequest,
    ResourceInclude, SearchResourcesRequest, SyncStatusRequest,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct SearchResourcesCommand {
    pub query: Option<String>,
    #[serde(default)]
    pub kinds: BTreeSet<String>,
    #[serde(default)]
    pub connector_types: BTreeSet<String>,
    #[serde(default)]
    pub health: BTreeSet<ResourceHealth>,
    #[serde(default)]
    pub freshness: BTreeSet<Freshness>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceIncludeCommand {
    Attributes,
    Relations,
    RecentChanges,
    ConnectorCoverage,
}

impl From<ResourceIncludeCommand> for ResourceInclude {
    fn from(value: ResourceIncludeCommand) -> Self {
        match value {
            ResourceIncludeCommand::Attributes => Self::Attributes,
            ResourceIncludeCommand::Relations => Self::Relations,
            ResourceIncludeCommand::RecentChanges => Self::RecentChanges,
            ResourceIncludeCommand::ConnectorCoverage => Self::ConnectorCoverage,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct GetResourceCommand {
    pub resource_id: String,
    #[serde(default)]
    pub include: BTreeSet<ResourceIncludeCommand>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct GetTopologyCommand {
    pub focus_resource_id: String,
    pub depth: Option<u8>,
    pub max_nodes: Option<usize>,
    pub max_edges: Option<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct RecentChangesCommand {
    pub since: Option<String>,
    pub resource_id: Option<String>,
    #[serde(default)]
    pub kinds: BTreeSet<String>,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct SyncStatusCommand {
    pub connection_id: String,
    pub recent_run_limit: Option<usize>,
}

pub struct DesktopQueryAdapter<S> {
    service: QueryService<S>,
}

impl<S> DesktopQueryAdapter<S> {
    pub fn new(service: QueryService<S>) -> Self {
        Self { service }
    }

    pub fn service(&self) -> &QueryService<S> {
        &self.service
    }
}

impl<S> DesktopQueryAdapter<S>
where
    S: QuerySource,
{
    pub fn search_resources(
        &self,
        request: SearchResourcesCommand,
    ) -> Result<ResourcePageDto, ErrorEnvelope> {
        self.service.search_resources(SearchResourcesRequest {
            query: request.query,
            kinds: request.kinds,
            connector_types: request.connector_types,
            health: request.health,
            freshness: request.freshness,
            labels: request.labels,
            limit: request.limit,
            cursor: request.cursor,
        })
    }

    pub fn get_resource(
        &self,
        request: GetResourceCommand,
    ) -> Result<ResourceDetailDto, ErrorEnvelope> {
        self.service.get_resource(GetResourceRequest {
            resource_id: request.resource_id,
            include: request.include.into_iter().map(Into::into).collect(),
        })
    }

    pub fn get_topology(&self, request: GetTopologyCommand) -> Result<TopologyDto, ErrorEnvelope> {
        self.service.get_topology(GetTopologyRequest {
            focus_resource_id: request.focus_resource_id,
            depth: request.depth,
            max_nodes: request.max_nodes,
            max_edges: request.max_edges,
        })
    }

    pub fn get_health_summary(&self) -> Result<HealthSummaryDto, ErrorEnvelope> {
        self.service.get_health_summary()
    }

    pub fn list_connections(&self) -> Result<ConnectionSnapshotDto, ErrorEnvelope> {
        self.service.list_connections()
    }

    pub fn get_recent_changes(
        &self,
        request: RecentChangesCommand,
    ) -> Result<ChangePageDto, ErrorEnvelope> {
        self.service.get_recent_changes(RecentChangesRequest {
            since: request.since,
            resource_id: request.resource_id,
            kinds: request.kinds,
            limit: request.limit,
            cursor: request.cursor,
        })
    }

    pub fn get_sync_status(
        &self,
        request: SyncStatusCommand,
    ) -> Result<SyncStatusDto, ErrorEnvelope> {
        self.service.get_sync_status(SyncStatusRequest {
            connection_id: request.connection_id,
            recent_run_limit: request.recent_run_limit,
        })
    }

    pub fn list_connector_coverage(&self) -> Result<ConnectorCoverageSnapshotDto, ErrorEnvelope> {
        self.service.list_connector_coverage()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ManualSyncResult {
    pub sync_run_id: String,
}

pub trait ManualSyncPort {
    type Error;

    fn enqueue_manual_sync(&self, connection_id: &str) -> Result<String, Self::Error>;
}

pub fn manual_sync<P>(port: &P, connection_id: &str) -> Result<ManualSyncResult, ErrorEnvelope>
where
    P: ManualSyncPort,
{
    let connection_id = connection_id.trim();
    if connection_id.is_empty() {
        return Err(command_error(
            "invalid_connection_id",
            "Connection identifier is required.",
            false,
        ));
    }
    port.enqueue_manual_sync(connection_id)
        .map(|sync_run_id| ManualSyncResult { sync_run_id })
        .map_err(|_| {
            command_error(
                "sync_enqueue_failed",
                "Manual sync could not be queued.",
                true,
            )
        })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryInvalidation {
    pub version: String,
    pub scopes: BTreeSet<String>,
}

impl QueryInvalidation {
    pub fn new(
        version: impl Into<String>,
        scopes: impl IntoIterator<Item = String>,
    ) -> Result<Self, ErrorEnvelope> {
        let version = version.into();
        if version.trim().is_empty() {
            return Err(command_error(
                "invalid_invalidation",
                "Invalidation version is required.",
                false,
            ));
        }
        Ok(Self {
            version,
            scopes: scopes
                .into_iter()
                .filter(|scope| !scope.is_empty())
                .collect(),
        })
    }
}

fn command_error(code: &str, message: &str, retryable: bool) -> ErrorEnvelope {
    ErrorEnvelope {
        schema_version: next_infra_query::dto::QUERY_DTO_SCHEMA_VERSION,
        code: code.into(),
        message: message.into(),
        retryable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeManualSync;

    impl ManualSyncPort for FakeManualSync {
        type Error = ();

        fn enqueue_manual_sync(&self, connection_id: &str) -> Result<String, Self::Error> {
            Ok(format!("fixture-run-{connection_id}"))
        }
    }

    #[test]
    fn manual_sync_returns_only_the_enqueued_run_identifier() {
        assert_eq!(
            manual_sync(&FakeManualSync, "fixture-connection").unwrap(),
            ManualSyncResult {
                sync_run_id: "fixture-run-fixture-connection".into()
            }
        );
    }

    #[test]
    fn invalidation_contains_only_version_and_minimal_scopes() {
        let event = QueryInvalidation::new(
            "fixture-version",
            [
                "resources".to_owned(),
                "resources".to_owned(),
                String::new(),
            ],
        )
        .unwrap();
        assert_eq!(event.version, "fixture-version");
        assert_eq!(event.scopes, BTreeSet::from(["resources".to_owned()]));
    }
}
