use crate::dto::*;
use std::collections::{BTreeMap, BTreeSet};

pub const DEFAULT_RESOURCE_LIMIT: usize = 25;
pub const MAX_RESOURCE_LIMIT: usize = 100;
pub const MAX_CONNECTIONS: usize = 200;
pub const DEFAULT_CHANGE_LIMIT: usize = 20;
pub const MAX_CHANGE_LIMIT: usize = 100;
pub const DEFAULT_TOPOLOGY_DEPTH: u8 = 1;
pub const MAX_TOPOLOGY_DEPTH: u8 = 3;
pub const DEFAULT_TOPOLOGY_NODES: usize = 100;
pub const DEFAULT_TOPOLOGY_EDGES: usize = 200;
pub const MAX_TOPOLOGY_NODES: usize = 200;
pub const MAX_TOPOLOGY_EDGES: usize = 400;
pub const DEFAULT_TIMELINE_LIMIT: usize = 50;
pub const MAX_TIMELINE_LIMIT: usize = 200;
pub const DEFAULT_RELATIONS_LIMIT: usize = 200;
pub const MAX_RELATIONS_LIMIT: usize = 400;

const CURSOR_PREFIX: &str = "niq1:";
const MAX_CURSOR_LENGTH: usize = 512;

pub type QueryResult<T> = Result<T, ErrorEnvelope>;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchResourcesRequest {
    pub query: Option<String>,
    pub kinds: BTreeSet<String>,
    pub connector_types: BTreeSet<String>,
    pub health: BTreeSet<ResourceHealth>,
    pub freshness: BTreeSet<Freshness>,
    pub labels: BTreeMap<String, String>,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RelationsForResourcesRequest {
    pub resource_ids: Vec<String>,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ResourceInclude {
    Attributes,
    Relations,
    RecentChanges,
    ConnectorCoverage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GetResourceRequest {
    pub resource_id: String,
    pub include: BTreeSet<ResourceInclude>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GetTopologyRequest {
    pub focus_resource_id: String,
    pub depth: Option<u8>,
    pub max_nodes: Option<usize>,
    pub max_edges: Option<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecentChangesRequest {
    pub since: Option<String>,
    pub resource_id: Option<String>,
    pub kinds: BTreeSet<String>,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncStatusRequest {
    pub connection_id: String,
    pub recent_run_limit: Option<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TimelineRequest {
    pub limit: Option<usize>,
    pub cursor: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceSearchPlan {
    pub query: Option<String>,
    pub kinds: BTreeSet<String>,
    pub connector_types: BTreeSet<String>,
    pub health: BTreeSet<ResourceHealth>,
    pub freshness: BTreeSet<Freshness>,
    pub labels: BTreeMap<String, String>,
    pub limit: usize,
    pub after: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecentChangesPlan {
    pub since: Option<String>,
    pub resource_id: Option<String>,
    pub kinds: BTreeSet<String>,
    pub limit: usize,
    pub after: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TopologyPlan {
    pub focus_resource_id: String,
    pub depth: u8,
    pub max_nodes: usize,
    pub max_edges: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimelinePlan {
    pub limit: usize,
    pub after: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SourcePage<T> {
    pub items: Vec<T>,
    pub next_after: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TimelineSourcePage {
    pub groups: Vec<TimelineGroupDto>,
    pub item_count: usize,
    pub next_after: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SourceSnapshot<T> {
    pub metadata: SnapshotMetadata,
    pub body: T,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResourceDetailBody {
    pub resource: ResourceDto,
    pub attributes: serde_json::Value,
    pub relations: Vec<RelationDto>,
    pub recent_changes: Vec<ChangeDto>,
    pub connector_coverage: Vec<ConnectorCoverageDto>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TopologyBody {
    pub nodes: Vec<ResourceDto>,
    pub edges: Vec<RelationDto>,
    pub frontier: Vec<TopologyFrontierDto>,
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HealthSummaryBody {
    pub resource_health: ResourceHealthCountsDto,
    pub freshness: FreshnessCountsDto,
    pub connector_health: ConnectorHealthCountsDto,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncStatusBody {
    pub connection: ConnectionDto,
    pub recent_runs: Vec<SyncRunDto>,
    pub next_scheduled_at: Option<String>,
}

pub trait QuerySource {
    type Error;

    fn search_resources(
        &self,
        plan: &ResourceSearchPlan,
    ) -> Result<SourceSnapshot<SourcePage<ResourceDto>>, Self::Error>;
    fn get_resource(
        &self,
        resource_id: &str,
        include: &BTreeSet<ResourceInclude>,
    ) -> Result<SourceSnapshot<Option<ResourceDetailBody>>, Self::Error>;
    fn get_topology(
        &self,
        plan: &TopologyPlan,
    ) -> Result<SourceSnapshot<Option<TopologyBody>>, Self::Error>;
    fn relations_for_resources(
        &self,
        resource_ids: &BTreeSet<String>,
        limit: usize,
        after: Option<&str>,
    ) -> Result<SourceSnapshot<SourcePage<RelationDto>>, Self::Error>;
    fn get_health_summary(&self) -> Result<SourceSnapshot<HealthSummaryBody>, Self::Error>;
    fn list_connections(&self) -> Result<SourceSnapshot<Vec<ConnectionDto>>, Self::Error>;
    fn get_recent_changes(
        &self,
        plan: &RecentChangesPlan,
    ) -> Result<SourceSnapshot<SourcePage<ChangeDto>>, Self::Error>;
    fn get_sync_status(
        &self,
        connection_id: &str,
        recent_run_limit: usize,
    ) -> Result<SourceSnapshot<Option<SyncStatusBody>>, Self::Error>;
    fn get_timeline(
        &self,
        plan: &TimelinePlan,
    ) -> Result<SourceSnapshot<TimelineSourcePage>, Self::Error>;
    fn list_connector_coverage(
        &self,
    ) -> Result<SourceSnapshot<Vec<ConnectorCoverageDto>>, Self::Error>;
}

pub struct QueryService<S> {
    source: S,
}

impl<S> QueryService<S> {
    pub fn new(source: S) -> Self {
        Self { source }
    }

    pub fn source(&self) -> &S {
        &self.source
    }

    pub fn into_source(self) -> S {
        self.source
    }
}

impl<S> QueryService<S>
where
    S: QuerySource,
{
    pub fn search_resources(
        &self,
        request: SearchResourcesRequest,
    ) -> QueryResult<ResourcePageDto> {
        let limit = bounded_limit(request.limit, DEFAULT_RESOURCE_LIMIT, MAX_RESOURCE_LIMIT)?;
        let plan = ResourceSearchPlan {
            query: clean_optional_text(request.query, "query")?,
            kinds: request.kinds,
            connector_types: request.connector_types,
            health: request.health,
            freshness: request.freshness,
            labels: request.labels,
            limit,
            after: decode_cursor(request.cursor.as_deref())?,
        };
        let snapshot = self
            .source
            .search_resources(&plan)
            .map_err(|_| source_error())?;
        let page = snapshot.body;
        validate_page_size(page.items.len(), limit)?;
        Ok(ResourcePageDto {
            metadata: snapshot.metadata,
            items: page.items,
            page_info: PageInfo::new(page.next_after.map(encode_cursor)),
        })
    }

    pub fn get_resource(&self, request: GetResourceRequest) -> QueryResult<ResourceDetailDto> {
        let resource_id = required_text(request.resource_id, "resource_id")?;
        let snapshot = self
            .source
            .get_resource(&resource_id, &request.include)
            .map_err(|_| source_error())?;
        let body = snapshot
            .body
            .ok_or_else(|| not_found("resource_not_found", "Resource was not found."))?;
        Ok(ResourceDetailDto {
            metadata: snapshot.metadata,
            resource: body.resource,
            attributes: body.attributes,
            relations: body.relations,
            recent_changes: body.recent_changes,
            connector_coverage: body.connector_coverage,
        })
    }

    pub fn get_topology(&self, request: GetTopologyRequest) -> QueryResult<TopologyDto> {
        let focus_resource_id = required_text(request.focus_resource_id, "focus_resource_id")?;
        let depth = bounded_u8(request.depth, DEFAULT_TOPOLOGY_DEPTH, MAX_TOPOLOGY_DEPTH)?;
        let max_nodes = bounded_limit(
            request.max_nodes,
            DEFAULT_TOPOLOGY_NODES,
            MAX_TOPOLOGY_NODES,
        )?;
        let max_edges = bounded_limit(
            request.max_edges,
            DEFAULT_TOPOLOGY_EDGES,
            MAX_TOPOLOGY_EDGES,
        )?;
        let plan = TopologyPlan {
            focus_resource_id: focus_resource_id.clone(),
            depth,
            max_nodes,
            max_edges,
        };
        let snapshot = self
            .source
            .get_topology(&plan)
            .map_err(|_| source_error())?;
        let body = snapshot
            .body
            .ok_or_else(|| not_found("resource_not_found", "Topology focus was not found."))?;
        if body.nodes.len() > max_nodes || body.edges.len() > max_edges {
            return Err(contract_error("query source exceeded topology limits"));
        }
        Ok(TopologyDto {
            metadata: snapshot.metadata,
            focus_resource_id,
            depth,
            nodes: body.nodes,
            edges: body.edges,
            frontier: body.frontier,
            truncated: body.truncated,
        })
    }

    pub fn get_relations_for_resources(
        &self,
        request: RelationsForResourcesRequest,
    ) -> QueryResult<RelationPageDto> {
        let limit = bounded_limit(request.limit, DEFAULT_RELATIONS_LIMIT, MAX_RELATIONS_LIMIT)?;
        let resource_ids = request.resource_ids.into_iter().collect::<BTreeSet<_>>();
        if resource_ids.is_empty() {
            return Err(contract_error("resource_ids must not be empty"));
        }
        if resource_ids.len() > MAX_RESOURCE_LIMIT {
            return Err(contract_error("too many resource ids"));
        }
        let snapshot = self
            .source
            .relations_for_resources(
                &resource_ids,
                limit,
                decode_cursor(request.cursor.as_deref())?.as_deref(),
            )
            .map_err(|_| source_error())?;
        let page = snapshot.body;
        validate_page_size(page.items.len(), limit)?;
        Ok(RelationPageDto {
            metadata: snapshot.metadata,
            items: page.items,
            page_info: PageInfo::new(page.next_after.map(encode_cursor)),
        })
    }

    pub fn get_health_summary(&self) -> QueryResult<HealthSummaryDto> {
        let snapshot = self
            .source
            .get_health_summary()
            .map_err(|_| source_error())?;
        let body = snapshot.body;
        Ok(HealthSummaryDto {
            metadata: snapshot.metadata,
            resource_health: body.resource_health,
            freshness: body.freshness,
            connector_health: body.connector_health,
        })
    }

    pub fn list_connections(&self) -> QueryResult<ConnectionSnapshotDto> {
        let snapshot = self.source.list_connections().map_err(|_| source_error())?;
        if snapshot.body.len() > MAX_CONNECTIONS {
            return Err(contract_error("query source exceeded connection limit"));
        }
        Ok(ConnectionSnapshotDto {
            metadata: snapshot.metadata,
            items: snapshot.body,
        })
    }

    pub fn get_recent_changes(&self, request: RecentChangesRequest) -> QueryResult<ChangePageDto> {
        let limit = bounded_limit(request.limit, DEFAULT_CHANGE_LIMIT, MAX_CHANGE_LIMIT)?;
        let plan = RecentChangesPlan {
            since: clean_optional_text(request.since, "since")?,
            resource_id: clean_optional_text(request.resource_id, "resource_id")?,
            kinds: request.kinds,
            limit,
            after: decode_cursor(request.cursor.as_deref())?,
        };
        let snapshot = self
            .source
            .get_recent_changes(&plan)
            .map_err(|_| source_error())?;
        let page = snapshot.body;
        validate_page_size(page.items.len(), limit)?;
        Ok(ChangePageDto {
            metadata: snapshot.metadata,
            items: page.items,
            page_info: PageInfo::new(page.next_after.map(encode_cursor)),
        })
    }

    pub fn get_sync_status(&self, request: SyncStatusRequest) -> QueryResult<SyncStatusDto> {
        let connection_id = required_text(request.connection_id, "connection_id")?;
        let limit = bounded_limit(request.recent_run_limit, 10, 100)?;
        let snapshot = self
            .source
            .get_sync_status(&connection_id, limit)
            .map_err(|_| source_error())?;
        let body = snapshot
            .body
            .ok_or_else(|| not_found("connection_not_found", "Connection was not found."))?;
        if body.recent_runs.len() > limit {
            return Err(contract_error("query source exceeded sync run limit"));
        }
        Ok(SyncStatusDto {
            metadata: snapshot.metadata,
            connection: body.connection,
            recent_runs: body.recent_runs,
            next_scheduled_at: body.next_scheduled_at,
        })
    }

    pub fn get_timeline(&self, request: TimelineRequest) -> QueryResult<TimelinePageDto> {
        let limit = bounded_limit(request.limit, DEFAULT_TIMELINE_LIMIT, MAX_TIMELINE_LIMIT)?;
        let plan = TimelinePlan {
            limit,
            after: decode_cursor(request.cursor.as_deref())?,
        };
        let snapshot = self
            .source
            .get_timeline(&plan)
            .map_err(|_| source_error())?;
        let page = snapshot.body;
        validate_page_size(page.item_count, limit)?;
        if page
            .groups
            .iter()
            .map(|group| group.items.len())
            .sum::<usize>()
            != page.item_count
        {
            return Err(contract_error("timeline group item count is inconsistent"));
        }
        Ok(TimelinePageDto {
            metadata: snapshot.metadata,
            groups: page.groups,
            page_info: PageInfo::new(page.next_after.map(encode_cursor)),
        })
    }

    pub fn list_connector_coverage(&self) -> QueryResult<ConnectorCoverageSnapshotDto> {
        let snapshot = self
            .source
            .list_connector_coverage()
            .map_err(|_| source_error())?;
        Ok(ConnectorCoverageSnapshotDto {
            metadata: snapshot.metadata,
            items: snapshot.body,
        })
    }
}

fn bounded_limit(value: Option<usize>, default: usize, maximum: usize) -> QueryResult<usize> {
    let value = value.unwrap_or(default);
    if value == 0 || value > maximum {
        return Err(invalid_request("limit is outside the supported range"));
    }
    Ok(value)
}

fn bounded_u8(value: Option<u8>, default: u8, maximum: u8) -> QueryResult<u8> {
    let value = value.unwrap_or(default);
    if value == 0 || value > maximum {
        return Err(invalid_request("depth is outside the supported range"));
    }
    Ok(value)
}

fn validate_page_size(actual: usize, limit: usize) -> QueryResult<()> {
    if actual > limit {
        Err(contract_error("query source exceeded page limit"))
    } else {
        Ok(())
    }
}

fn required_text(value: String, field: &str) -> QueryResult<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > 512 || trimmed.chars().any(char::is_control) {
        return Err(invalid_request(&format!("{field} is invalid")));
    }
    Ok(trimmed.to_owned())
}

fn clean_optional_text(value: Option<String>, field: &str) -> QueryResult<Option<String>> {
    value.map(|value| required_text(value, field)).transpose()
}

fn decode_cursor(cursor: Option<&str>) -> QueryResult<Option<String>> {
    let Some(cursor) = cursor else {
        return Ok(None);
    };
    if cursor.len() > MAX_CURSOR_LENGTH || !cursor.starts_with(CURSOR_PREFIX) {
        return Err(invalid_request("cursor is invalid"));
    }
    let value = &cursor[CURSOR_PREFIX.len()..];
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err(invalid_request("cursor is invalid"));
    }
    Ok(Some(value.to_owned()))
}

fn encode_cursor(value: String) -> String {
    format!("{CURSOR_PREFIX}{value}")
}

fn invalid_request(message: &str) -> ErrorEnvelope {
    ErrorEnvelope {
        schema_version: QUERY_DTO_SCHEMA_VERSION,
        code: "invalid_request".into(),
        message: message.into(),
        retryable: false,
    }
}

fn not_found(code: &str, message: &str) -> ErrorEnvelope {
    ErrorEnvelope {
        schema_version: QUERY_DTO_SCHEMA_VERSION,
        code: code.into(),
        message: message.into(),
        retryable: false,
    }
}

fn source_error() -> ErrorEnvelope {
    ErrorEnvelope {
        schema_version: QUERY_DTO_SCHEMA_VERSION,
        code: "query_source_unavailable".into(),
        message: "The local query snapshot is temporarily unavailable.".into(),
        retryable: true,
    }
}

fn contract_error(message: &str) -> ErrorEnvelope {
    ErrorEnvelope {
        schema_version: QUERY_DTO_SCHEMA_VERSION,
        code: "query_contract_violation".into(),
        message: message.into(),
        retryable: false,
    }
}
