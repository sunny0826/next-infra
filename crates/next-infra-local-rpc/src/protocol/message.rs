use next_infra_query::dto::{
    ChangePageDto, ConnectorCoverageSnapshotDto, Freshness, HealthSummaryDto, ResourceDetailDto,
    ResourceHealth, ResourcePageDto, SyncStatusDto, TopologyDto,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use super::error::RpcError;
use super::handshake::Capability;
use super::{MAX_REQUEST_ID_BYTES, validate_request_id};

/// Structured caller identity.  It intentionally contains no executable path,
/// arbitrary method name, or authorization flag.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Caller {
    Bridge {
        bridge_version: String,
        release_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        diagnostic: Option<String>,
    },
    Test {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        diagnostic: Option<String>,
    },
}

impl Caller {
    pub fn bridge(bridge_version: impl Into<String>, release_id: impl Into<String>) -> Self {
        Self::Bridge {
            bridge_version: bridge_version.into(),
            release_id: release_id.into(),
            diagnostic: None,
        }
    }

    pub fn test(name: impl Into<String>) -> Self {
        Self::Test {
            name: name.into(),
            diagnostic: None,
        }
    }

    pub fn with_diagnostic(self, diagnostic: impl Into<String>) -> Self {
        let diagnostic = Some(diagnostic.into());
        match self {
            Self::Bridge {
                bridge_version,
                release_id,
                ..
            } => Self::Bridge {
                bridge_version,
                release_id,
                diagnostic,
            },
            Self::Test { name, .. } => Self::Test { name, diagnostic },
        }
    }

    fn validate(&self) -> Result<(), RpcError> {
        let value_is_valid =
            |value: &str| !value.trim().is_empty() && !value.chars().any(char::is_control);
        match self {
            Self::Bridge {
                bridge_version,
                release_id,
                diagnostic,
            } => {
                if !value_is_valid(bridge_version)
                    || !value_is_valid(release_id)
                    || diagnostic
                        .as_deref()
                        .is_some_and(|value| value.chars().any(char::is_control))
                {
                    return Err(RpcError::invalid_frame("caller identity is invalid"));
                }
            }
            Self::Test { name, diagnostic } => {
                if !value_is_valid(name)
                    || diagnostic
                        .as_deref()
                        .is_some_and(|value| value.chars().any(char::is_control))
                {
                    return Err(RpcError::invalid_frame("caller identity is invalid"));
                }
            }
        }
        Ok(())
    }
}

/// Include flags for the get-resource query.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceInclude {
    Attributes,
    Relations,
    RecentChanges,
    ConnectorCoverage,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchResourcesQuery {
    #[serde(default)]
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
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub cursor: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GetResourceQuery {
    pub resource_id: String,
    #[serde(default)]
    pub include: BTreeSet<ResourceInclude>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GetTopologyQuery {
    pub focus_resource_id: String,
    #[serde(default)]
    pub depth: Option<u8>,
    #[serde(default)]
    pub max_nodes: Option<usize>,
    #[serde(default)]
    pub max_edges: Option<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecentChangesQuery {
    #[serde(default)]
    pub since: Option<String>,
    #[serde(default)]
    pub resource_id: Option<String>,
    #[serde(default)]
    pub kinds: BTreeSet<String>,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub cursor: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncStatusQuery {
    pub connection_id: String,
    #[serde(default)]
    pub recent_run_limit: Option<usize>,
}

/// Closed set of the seven read-only query operations.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum QueryRequest {
    SearchResources(SearchResourcesQuery),
    GetResource(GetResourceQuery),
    GetTopology(GetTopologyQuery),
    GetHealthSummary,
    GetRecentChanges(RecentChangesQuery),
    GetSyncStatus(SyncStatusQuery),
    ListConnectorCoverage,
}

impl QueryRequest {
    pub fn capability(&self) -> Capability {
        match self {
            Self::SearchResources(_) => Capability::SearchResources,
            Self::GetResource(_) => Capability::GetResource,
            Self::GetTopology(_) => Capability::GetTopology,
            Self::GetHealthSummary => Capability::GetHealthSummary,
            Self::GetRecentChanges(_) => Capability::GetRecentChanges,
            Self::GetSyncStatus(_) => Capability::GetSyncStatus,
            Self::ListConnectorCoverage => Capability::ListConnectorCoverage,
        }
    }
}

impl From<ResourceInclude> for next_infra_query::service::ResourceInclude {
    fn from(include: ResourceInclude) -> Self {
        match include {
            ResourceInclude::Attributes => Self::Attributes,
            ResourceInclude::Relations => Self::Relations,
            ResourceInclude::RecentChanges => Self::RecentChanges,
            ResourceInclude::ConnectorCoverage => Self::ConnectorCoverage,
        }
    }
}

impl From<SearchResourcesQuery> for next_infra_query::service::SearchResourcesRequest {
    fn from(query: SearchResourcesQuery) -> Self {
        Self {
            query: query.query,
            kinds: query.kinds,
            connector_types: query.connector_types,
            health: query.health,
            freshness: query.freshness,
            labels: query.labels,
            limit: query.limit,
            cursor: query.cursor,
        }
    }
}

impl From<GetResourceQuery> for next_infra_query::service::GetResourceRequest {
    fn from(query: GetResourceQuery) -> Self {
        Self {
            resource_id: query.resource_id,
            include: query.include.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<GetTopologyQuery> for next_infra_query::service::GetTopologyRequest {
    fn from(query: GetTopologyQuery) -> Self {
        Self {
            focus_resource_id: query.focus_resource_id,
            depth: query.depth,
            max_nodes: query.max_nodes,
            max_edges: query.max_edges,
        }
    }
}

impl From<RecentChangesQuery> for next_infra_query::service::RecentChangesRequest {
    fn from(query: RecentChangesQuery) -> Self {
        Self {
            since: query.since,
            resource_id: query.resource_id,
            kinds: query.kinds,
            limit: query.limit,
            cursor: query.cursor,
        }
    }
}

impl From<SyncStatusQuery> for next_infra_query::service::SyncStatusRequest {
    fn from(query: SyncStatusQuery) -> Self {
        Self {
            connection_id: query.connection_id,
            recent_run_limit: query.recent_run_limit,
        }
    }
}

/// DTO-backed response variants corresponding one-to-one with QueryRequest.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum QueryResponse {
    SearchResources(ResourcePageDto),
    GetResource(ResourceDetailDto),
    GetTopology(TopologyDto),
    GetHealthSummary(HealthSummaryDto),
    GetRecentChanges(ChangePageDto),
    GetSyncStatus(SyncStatusDto),
    ListConnectorCoverage(ConnectorCoverageSnapshotDto),
}

impl QueryResponse {
    pub fn capability(&self) -> Capability {
        match self {
            Self::SearchResources(_) => Capability::SearchResources,
            Self::GetResource(_) => Capability::GetResource,
            Self::GetTopology(_) => Capability::GetTopology,
            Self::GetHealthSummary(_) => Capability::GetHealthSummary,
            Self::GetRecentChanges(_) => Capability::GetRecentChanges,
            Self::GetSyncStatus(_) => Capability::GetSyncStatus,
            Self::ListConnectorCoverage(_) => Capability::ListConnectorCoverage,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RawRequestEnvelope")]
pub struct RequestEnvelope {
    pub request_id: String,
    pub caller: Caller,
    pub query: QueryRequest,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRequestEnvelope {
    request_id: String,
    caller: Caller,
    query: QueryRequest,
}

impl TryFrom<RawRequestEnvelope> for RequestEnvelope {
    type Error = RpcError;

    fn try_from(raw: RawRequestEnvelope) -> Result<Self, Self::Error> {
        Self::new(raw.request_id, raw.caller, raw.query)
    }
}

impl RequestEnvelope {
    pub fn new(
        request_id: impl Into<String>,
        caller: Caller,
        query: QueryRequest,
    ) -> Result<Self, RpcError> {
        let envelope = Self {
            request_id: request_id.into(),
            caller,
            query,
        };
        envelope.validate()?;
        Ok(envelope)
    }

    pub fn validate(&self) -> Result<(), RpcError> {
        validate_request_id(&self.request_id)?;
        self.caller.validate()?;
        Ok(())
    }

    pub fn request_id_is_at_limit(&self) -> bool {
        self.request_id.len() == MAX_REQUEST_ID_BYTES
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ResponseBody {
    Query(Box<QueryResponse>),
    Error(RpcError),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RawResponseEnvelope")]
pub struct ResponseEnvelope {
    pub request_id: String,
    pub body: ResponseBody,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawResponseEnvelope {
    request_id: String,
    body: ResponseBody,
}

impl TryFrom<RawResponseEnvelope> for ResponseEnvelope {
    type Error = RpcError;

    fn try_from(raw: RawResponseEnvelope) -> Result<Self, Self::Error> {
        let envelope = Self {
            request_id: raw.request_id,
            body: raw.body,
        };
        envelope.validate()?;
        Ok(envelope)
    }
}

impl ResponseEnvelope {
    pub fn success(
        request_id: impl Into<String>,
        response: QueryResponse,
    ) -> Result<Self, RpcError> {
        let envelope = Self {
            request_id: request_id.into(),
            body: ResponseBody::Query(Box::new(response)),
        };
        envelope.validate()?;
        Ok(envelope)
    }

    pub fn error(request_id: impl Into<String>, error: RpcError) -> Result<Self, RpcError> {
        let envelope = Self {
            request_id: request_id.into(),
            body: ResponseBody::Error(error),
        };
        envelope.validate()?;
        Ok(envelope)
    }

    pub fn validate(&self) -> Result<(), RpcError> {
        validate_request_id(&self.request_id)
    }
}
