use std::collections::BTreeMap;

use next_infra_local_rpc::protocol::{
    GetResourceQuery, GetTopologyQuery, RecentChangesQuery, ResourceInclude, SearchResourcesQuery,
    SyncStatusQuery,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResourceHealthInput {
    Healthy,
    Degraded,
    Unhealthy,
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessInput {
    Fresh,
    Stale,
    Expired,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResourceIncludeInput {
    Attributes,
    Relations,
    RecentChanges,
    ConnectorCoverage,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchResourcesInput {
    #[schemars(length(max = 512))]
    pub query: Option<String>,
    #[schemars(length(max = 100), inner(length(min = 1, max = 512)))]
    #[serde(default)]
    pub kinds: Vec<String>,
    #[schemars(length(max = 100), inner(length(min = 1, max = 512)))]
    #[serde(default)]
    pub connector_types: Vec<String>,
    #[schemars(length(max = 4))]
    #[serde(default)]
    pub health: Vec<ResourceHealthInput>,
    #[schemars(length(max = 3))]
    #[serde(default)]
    pub freshness: Vec<FreshnessInput>,
    #[schemars(length(max = 100))]
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    #[schemars(range(min = 1, max = 100))]
    pub limit: Option<usize>,
    #[schemars(length(max = 512))]
    pub cursor: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetResourceInput {
    #[schemars(length(min = 1, max = 512))]
    pub resource_id: String,
    #[schemars(length(max = 4))]
    #[serde(default)]
    pub include: Vec<ResourceIncludeInput>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetTopologyInput {
    #[schemars(length(min = 1, max = 512))]
    pub focus_resource_id: String,
    #[schemars(range(min = 1, max = 3))]
    pub depth: Option<u8>,
    #[schemars(range(min = 1, max = 200))]
    pub max_nodes: Option<usize>,
    #[schemars(range(min = 1, max = 400))]
    pub max_edges: Option<usize>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecentChangesInput {
    #[schemars(length(max = 512))]
    pub since: Option<String>,
    #[schemars(length(max = 512))]
    pub resource_id: Option<String>,
    #[schemars(length(max = 100), inner(length(min = 1, max = 512)))]
    #[serde(default)]
    pub kinds: Vec<String>,
    #[schemars(range(min = 1, max = 100))]
    pub limit: Option<usize>,
    #[schemars(length(max = 512))]
    pub cursor: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SyncStatusInput {
    #[schemars(length(min = 1, max = 512))]
    pub connection_id: String,
    #[schemars(range(min = 1, max = 100))]
    pub recent_run_limit: Option<usize>,
}

impl From<SearchResourcesInput> for SearchResourcesQuery {
    fn from(input: SearchResourcesInput) -> Self {
        Self {
            query: input.query,
            kinds: input.kinds.into_iter().collect(),
            connector_types: input.connector_types.into_iter().collect(),
            health: input.health.into_iter().map(Into::into).collect(),
            freshness: input.freshness.into_iter().map(Into::into).collect(),
            labels: input.labels,
            limit: input.limit,
            cursor: input.cursor,
        }
    }
}

impl From<GetResourceInput> for GetResourceQuery {
    fn from(input: GetResourceInput) -> Self {
        Self {
            resource_id: input.resource_id,
            include: input.include.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<GetTopologyInput> for GetTopologyQuery {
    fn from(input: GetTopologyInput) -> Self {
        Self {
            focus_resource_id: input.focus_resource_id,
            depth: input.depth,
            max_nodes: input.max_nodes,
            max_edges: input.max_edges,
        }
    }
}

impl From<RecentChangesInput> for RecentChangesQuery {
    fn from(input: RecentChangesInput) -> Self {
        Self {
            since: input.since,
            resource_id: input.resource_id,
            kinds: input.kinds.into_iter().collect(),
            limit: input.limit,
            cursor: input.cursor,
        }
    }
}

impl From<SyncStatusInput> for SyncStatusQuery {
    fn from(input: SyncStatusInput) -> Self {
        Self {
            connection_id: input.connection_id,
            recent_run_limit: input.recent_run_limit,
        }
    }
}

impl From<ResourceHealthInput> for next_infra_local_rpc::protocol::ResourceHealth {
    fn from(input: ResourceHealthInput) -> Self {
        match input {
            ResourceHealthInput::Healthy => Self::Healthy,
            ResourceHealthInput::Degraded => Self::Degraded,
            ResourceHealthInput::Unhealthy => Self::Unhealthy,
            ResourceHealthInput::Unknown => Self::Unknown,
        }
    }
}

impl From<FreshnessInput> for next_infra_local_rpc::protocol::Freshness {
    fn from(input: FreshnessInput) -> Self {
        match input {
            FreshnessInput::Fresh => Self::Fresh,
            FreshnessInput::Stale => Self::Stale,
            FreshnessInput::Expired => Self::Expired,
        }
    }
}

impl From<ResourceIncludeInput> for ResourceInclude {
    fn from(input: ResourceIncludeInput) -> Self {
        match input {
            ResourceIncludeInput::Attributes => Self::Attributes,
            ResourceIncludeInput::Relations => Self::Relations,
            ResourceIncludeInput::RecentChanges => Self::RecentChanges,
            ResourceIncludeInput::ConnectorCoverage => Self::ConnectorCoverage,
        }
    }
}
