//! Runtime-owned committed query source.
//!
//! The query crate owns the public request/response contract.  This module is
//! deliberately the small adapter between that contract and the Store query
//! projections: it snapshots the runtime context once, reads committed Store
//! projections through the shared Store handle, and maps Core values into
//! user-safe query DTOs.

use crate::SharedStore;
use next_infra_connector_catalog::ConnectorCoverageSnapshot;
use next_infra_core::{
    Change, ChangeSubject, ConnectorCoverageLevel, ConnectorHealth as CoreConnectorHealth,
    ConnectorType, CoverageGapReason, DomainError, Freshness as CoreFreshness, Lifecycle,
    OriginRef, RelationEvidence, Resource, ResourceHealth as CoreResourceHealth, ResourceId,
    StoreReader, SyncCoverage, SyncMode, SyncRun, SyncRunStatus, SyncTrigger, Timestamp,
};
use next_infra_query::dto::{
    ChangeOriginDto, ChangeSubjectDto, ConnectionDto, ConnectorCoverageDto,
    ConnectorCoverageLevelDto, ConnectorHealth as QueryConnectorHealth, ConnectorHealthCountsDto,
    EvidenceType, FieldChangeDto, Freshness as QueryFreshness, FreshnessCountsDto,
    FrontierDirectionDto, Lifecycle as QueryLifecycle, RelationDto, RelationEvidenceDto,
    ResourceDto, ResourceHealth as QueryResourceHealth, ResourceHealthCountsDto, SnapshotMetadata,
    SyncCoverageDto, SyncModeDto, SyncRunCountsDto, SyncRunDto, SyncRunErrorDto, SyncRunStatusDto,
    SyncTriggerDto, TimelineGroupDto, TimelineItemDto, TimelineOriginDto, TimelineVersionLinkDto,
    TopologyFrontierDto,
};
use next_infra_query::service::{
    HealthSummaryBody, QuerySource, RecentChangesPlan, ResourceDetailBody, ResourceInclude,
    ResourceSearchPlan, SourcePage, SourceSnapshot, SyncStatusBody, TimelinePlan,
    TimelineSourcePage, TopologyBody, TopologyPlan,
};
use next_infra_store::{
    FreshnessCutoffs, HealthProjection, ProjectedConnection, ProjectedRelation, ProjectionMetadata,
    RecentChangesProjectionPlan, ResourceDetailProjection, ResourceProjectionPlan, StoreError,
    SyncStatusProjection, TimelineVersionLinkProjection,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::{Arc, RwLock, RwLockReadGuard};

/// A single schedule entry used by the immutable query context.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuerySchedule {
    pub interval_millis: u64,
    pub next_scheduled_at: Option<Timestamp>,
}

impl QuerySchedule {
    pub fn new(
        interval_millis: u64,
        next_scheduled_at: Option<Timestamp>,
    ) -> Result<Self, QueryContextError> {
        if interval_millis == 0 {
            return Err(QueryContextError::InvalidInterval);
        }
        Ok(Self {
            interval_millis,
            next_scheduled_at,
        })
    }

    pub const fn interval_millis(&self) -> u64 {
        self.interval_millis
    }

    pub const fn next_scheduled_at(&self) -> Option<Timestamp> {
        self.next_scheduled_at
    }
}

/// Input accepted by [`QueryContextSnapshot::new`].
pub trait IntoQueryScheduleEntry {
    fn into_query_schedule_entry(
        self,
    ) -> Result<(next_infra_core::ConnectionId, QuerySchedule), QueryContextError>;
}

impl IntoQueryScheduleEntry for (next_infra_core::ConnectionId, QuerySchedule) {
    fn into_query_schedule_entry(
        self,
    ) -> Result<(next_infra_core::ConnectionId, QuerySchedule), QueryContextError> {
        QuerySchedule::new(self.1.interval_millis, self.1.next_scheduled_at)
            .map(|schedule| (self.0, schedule))
    }
}

impl IntoQueryScheduleEntry for (next_infra_core::ConnectionId, u64) {
    fn into_query_schedule_entry(
        self,
    ) -> Result<(next_infra_core::ConnectionId, QuerySchedule), QueryContextError> {
        QuerySchedule::new(self.1, None).map(|schedule| (self.0, schedule))
    }
}

impl IntoQueryScheduleEntry for (next_infra_core::ConnectionId, u64, Option<Timestamp>) {
    fn into_query_schedule_entry(
        self,
    ) -> Result<(next_infra_core::ConnectionId, QuerySchedule), QueryContextError> {
        QuerySchedule::new(self.1, self.2).map(|schedule| (self.0, schedule))
    }
}

/// Snapshot of all time-dependent inputs used by one query.
///
/// The value is immutable after construction.  A new value is published via
/// [`QueryContextRefreshHandle::refresh`], so one source call never observes
/// two different clocks or schedule revisions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryContextSnapshot {
    pub evaluated_at: Timestamp,
    pub query_context_revision: u64,
    pub schedules: BTreeMap<next_infra_core::ConnectionId, QuerySchedule>,
}

impl QueryContextSnapshot {
    pub fn new<I, E>(
        evaluated_at: Timestamp,
        query_context_revision: u64,
        schedules: I,
    ) -> Result<Self, QueryContextError>
    where
        I: IntoIterator<Item = E>,
        E: IntoQueryScheduleEntry,
    {
        let schedules = schedules
            .into_iter()
            .map(IntoQueryScheduleEntry::into_query_schedule_entry)
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        Ok(Self {
            evaluated_at,
            query_context_revision,
            schedules,
        })
    }

    pub fn empty(evaluated_at: Timestamp, query_context_revision: u64) -> Self {
        Self {
            evaluated_at,
            query_context_revision,
            schedules: BTreeMap::new(),
        }
    }

    pub fn from_intervals<I>(
        evaluated_at: Timestamp,
        query_context_revision: u64,
        intervals: I,
    ) -> Result<Self, QueryContextError>
    where
        I: IntoIterator<Item = (next_infra_core::ConnectionId, u64, Option<Timestamp>)>,
    {
        let schedules = intervals
            .into_iter()
            .map(|(connection_id, interval_millis, next_scheduled_at)| {
                QuerySchedule::new(interval_millis, next_scheduled_at)
                    .map(|schedule| (connection_id, schedule))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        Self::new(evaluated_at, query_context_revision, schedules)
    }

    pub fn schedule(
        &self,
        connection_id: &next_infra_core::ConnectionId,
    ) -> Option<&QuerySchedule> {
        self.schedules.get(connection_id)
    }

    pub const fn evaluated_at(&self) -> Timestamp {
        self.evaluated_at
    }

    pub const fn query_context_revision(&self) -> u64 {
        self.query_context_revision
    }

    pub fn interval_millis(&self, connection_id: &next_infra_core::ConnectionId) -> Option<u64> {
        self.schedule(connection_id)
            .map(|schedule| schedule.interval_millis)
    }
}

/// Errors raised while constructing or refreshing a query context.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryContextError {
    InvalidInterval,
    RevisionMustIncrease,
}

impl fmt::Display for QueryContextError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInterval => formatter.write_str("query schedule interval is invalid"),
            Self::RevisionMustIncrease => {
                formatter.write_str("query context revision must increase")
            }
        }
    }
}

impl std::error::Error for QueryContextError {}

/// Cloneable refresh handle for the immutable query context.
#[derive(Clone)]
pub struct QueryContextRefreshHandle {
    inner: Arc<RwLock<QueryContextSnapshot>>,
}

/// Short alias retained for callers that refer to the handle as a context
/// handle rather than a refresh handle.
pub type QueryContextHandle = QueryContextRefreshHandle;

impl QueryContextRefreshHandle {
    pub fn new(snapshot: QueryContextSnapshot) -> Self {
        Self {
            inner: Arc::new(RwLock::new(snapshot)),
        }
    }

    pub fn from_arc(inner: Arc<RwLock<QueryContextSnapshot>>) -> Self {
        Self { inner }
    }

    pub fn arc(&self) -> Arc<RwLock<QueryContextSnapshot>> {
        self.inner.clone()
    }

    pub fn snapshot(&self) -> Result<QueryContextSnapshot, QueryContextError> {
        Ok(self.read()?.clone())
    }

    pub fn refresh(&self, snapshot: QueryContextSnapshot) -> Result<(), QueryContextError> {
        let mut current = self.write()?;
        if snapshot.query_context_revision <= current.query_context_revision {
            return Err(QueryContextError::RevisionMustIncrease);
        }
        *current = snapshot;
        Ok(())
    }

    pub fn refresh_snapshot(
        &self,
        snapshot: QueryContextSnapshot,
    ) -> Result<(), QueryContextError> {
        self.refresh(snapshot)
    }

    pub fn revision(&self) -> Result<u64, QueryContextError> {
        Ok(self.read()?.query_context_revision)
    }

    fn read(&self) -> Result<RwLockReadGuard<'_, QueryContextSnapshot>, QueryContextError> {
        self.inner
            .read()
            .map_err(|_| QueryContextError::RevisionMustIncrease)
    }

    fn write(
        &self,
    ) -> Result<std::sync::RwLockWriteGuard<'_, QueryContextSnapshot>, QueryContextError> {
        self.inner
            .write()
            .map_err(|_| QueryContextError::RevisionMustIncrease)
    }
}

impl From<QueryContextSnapshot> for QueryContextRefreshHandle {
    fn from(snapshot: QueryContextSnapshot) -> Self {
        Self::new(snapshot)
    }
}

impl From<Arc<RwLock<QueryContextSnapshot>>> for QueryContextRefreshHandle {
    fn from(inner: Arc<RwLock<QueryContextSnapshot>>) -> Self {
        Self::from_arc(inner)
    }
}

/// Immutable descriptor catalog consumed by the query source.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConnectorCatalogSnapshot {
    pub connectors: Vec<ConnectorCoverageSnapshot>,
}

impl ConnectorCatalogSnapshot {
    pub fn new<I>(connectors: I) -> Self
    where
        I: IntoIterator<Item = ConnectorCoverageSnapshot>,
    {
        let mut connectors = connectors.into_iter().collect::<Vec<_>>();
        connectors.sort_by(|left, right| {
            left.connector_type
                .cmp(&right.connector_type)
                .then(left.connector_version.cmp(&right.connector_version))
        });
        Self { connectors }
    }

    pub fn fingerprint(&self) -> String {
        let mut versions = self
            .connectors
            .iter()
            .map(|connector| {
                format!(
                    "{}@{}",
                    connector.connector_type.as_str(),
                    connector.connector_version
                )
            })
            .collect::<Vec<_>>();
        versions.sort();
        versions.join(",")
    }

    fn coverage(&self, connector_type: Option<&ConnectorType>) -> Vec<ConnectorCoverageDto> {
        let mut items = self
            .connectors
            .iter()
            .filter(|connector| {
                connector_type
                    .map(|expected| &connector.connector_type == expected)
                    .unwrap_or(true)
            })
            .flat_map(|connector| {
                connector.modules.iter().map(|module| ConnectorCoverageDto {
                    connector_type: connector.connector_type.as_str().to_owned(),
                    connector_version: connector.connector_version.clone(),
                    module: module.module.clone(),
                    level: coverage_level(module.level),
                    reason: module.reason.clone(),
                })
            })
            .collect::<Vec<_>>();
        items.sort_by(|left, right| {
            left.connector_type
                .cmp(&right.connector_type)
                .then(left.connector_version.cmp(&right.connector_version))
                .then(left.module.cmp(&right.module))
        });
        items
    }
}

impl From<Vec<ConnectorCoverageSnapshot>> for ConnectorCatalogSnapshot {
    fn from(connectors: Vec<ConnectorCoverageSnapshot>) -> Self {
        Self::new(connectors)
    }
}

/// Internal source failure. QueryService deliberately redacts this into its
/// stable `query_source_unavailable` envelope.
#[derive(Debug)]
pub enum QuerySourceError {
    Store(StoreError),
    Context(QueryContextError),
    Contract(String),
}

impl fmt::Display for QuerySourceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "store query failed: {error}"),
            Self::Context(error) => write!(formatter, "query context failed: {error}"),
            Self::Contract(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for QuerySourceError {}

impl From<StoreError> for QuerySourceError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

impl From<QueryContextError> for QuerySourceError {
    fn from(error: QueryContextError) -> Self {
        Self::Context(error)
    }
}

/// QuerySource backed by the committed projection of the Runtime's shared
/// SQLite owner.
#[derive(Clone)]
pub struct CommittedQuerySource {
    store: SharedStore,
    catalog: ConnectorCatalogSnapshot,
    context: QueryContextRefreshHandle,
}

impl CommittedQuerySource {
    pub fn new<C, Q>(store: SharedStore, catalog: C, context: Q) -> Self
    where
        C: Into<ConnectorCatalogSnapshot>,
        Q: Into<QueryContextRefreshHandle>,
    {
        Self {
            store,
            catalog: catalog.into(),
            context: context.into(),
        }
    }

    pub fn from_shared_store<C, Q>(store: SharedStore, catalog: C, context: Q) -> Self
    where
        C: Into<ConnectorCatalogSnapshot>,
        Q: Into<QueryContextRefreshHandle>,
    {
        Self::new(store, catalog, context)
    }

    pub fn store(&self) -> &SharedStore {
        &self.store
    }

    pub fn catalog(&self) -> &ConnectorCatalogSnapshot {
        &self.catalog
    }

    pub fn context_handle(&self) -> QueryContextRefreshHandle {
        self.context.clone()
    }

    pub fn refresh_context(&self, snapshot: QueryContextSnapshot) -> Result<(), QueryContextError> {
        self.context.refresh(snapshot)
    }

    fn context_snapshot(&self) -> Result<QueryContextSnapshot, QuerySourceError> {
        self.context.snapshot().map_err(Into::into)
    }

    fn cutoffs(
        &self,
        context: &QueryContextSnapshot,
    ) -> Result<BTreeMap<next_infra_core::ConnectionId, FreshnessCutoffs>, QuerySourceError> {
        context
            .schedules
            .iter()
            .map(|(connection_id, schedule)| {
                let interval = i64::try_from(schedule.interval_millis).unwrap_or(i64::MAX);
                let evaluated = context.evaluated_at.unix_millis();
                let triple = interval.checked_mul(3).unwrap_or(i64::MAX);
                Ok((
                    connection_id.clone(),
                    FreshnessCutoffs {
                        fresh_after_millis: evaluated.saturating_sub(interval),
                        expired_after_millis: evaluated.saturating_sub(triple),
                    },
                ))
            })
            .collect()
    }

    fn metadata(
        &self,
        projection: ProjectionMetadata,
        context: &QueryContextSnapshot,
    ) -> SnapshotMetadata {
        SnapshotMetadata {
            schema_version: next_infra_query::dto::QUERY_DTO_SCHEMA_VERSION,
            snapshot_version: format!(
                "nis1:{}:{}:{}:{}",
                projection.committed_revision,
                self.catalog.fingerprint(),
                context.evaluated_at.unix_millis(),
                context.query_context_revision
            ),
            generated_at: format_timestamp(context.evaluated_at),
        }
    }
}

impl QuerySource for CommittedQuerySource {
    type Error = QuerySourceError;

    fn search_resources(
        &self,
        plan: &ResourceSearchPlan,
    ) -> Result<SourceSnapshot<SourcePage<ResourceDto>>, Self::Error> {
        let context = self.context_snapshot()?;
        let cutoffs = self.cutoffs(&context)?;
        let projection = self
            .store
            .read(|store| {
                store.query_resources(&ResourceProjectionPlan {
                    query: plan.query.clone(),
                    kinds: plan.kinds.clone(),
                    connector_types: plan.connector_types.clone(),
                    health: plan
                        .health
                        .iter()
                        .copied()
                        .map(core_resource_health)
                        .collect(),
                    freshness: plan.freshness.iter().copied().map(core_freshness).collect(),
                    labels: plan.labels.clone(),
                    cutoffs,
                    limit: plan.limit,
                    after: plan.after.clone(),
                })
            })
            .map_err(QuerySourceError::from)?;
        let metadata = self.metadata(projection.metadata, &context);
        let body = SourcePage {
            items: projection
                .body
                .items
                .into_iter()
                .map(|item| resource_dto(&item.resource, item.freshness))
                .collect::<Result<Vec<_>, _>>()?,
            next_after: projection.body.next_after,
        };
        Ok(SourceSnapshot { metadata, body })
    }

    fn get_resource(
        &self,
        resource_id: &str,
        include: &BTreeSet<ResourceInclude>,
    ) -> Result<SourceSnapshot<Option<ResourceDetailBody>>, Self::Error> {
        let context = self.context_snapshot()?;
        let id = ResourceId::new(resource_id.to_owned())
            .map_err(|error| QuerySourceError::Contract(error.to_string()))?;
        let (projection, connector_type) = self
            .store
            .read(|store| {
                let projection = store.query_resource_detail(&id)?;
                let connector_type = projection
                    .body
                    .as_ref()
                    .map(|detail| store.get_connection(&detail.resource.connection_id))
                    .transpose()?
                    .flatten()
                    .map(|connection| connection.connector_type);
                Ok((projection, connector_type))
            })
            .map_err(QuerySourceError::from)?;
        let metadata = self.metadata(projection.metadata, &context);
        let body = projection
            .body
            .map(|detail| {
                resource_detail_body(
                    detail,
                    include,
                    &context,
                    connector_type.as_ref(),
                    &self.catalog,
                )
            })
            .transpose()?;
        Ok(SourceSnapshot { metadata, body })
    }

    fn get_topology(
        &self,
        plan: &TopologyPlan,
    ) -> Result<SourceSnapshot<Option<TopologyBody>>, Self::Error> {
        let context = self.context_snapshot()?;
        if plan.depth == 0 || plan.max_nodes == 0 || plan.max_edges == 0 {
            return Err(QuerySourceError::Contract(
                "topology bounds must be positive".into(),
            ));
        }
        let focus_id = ResourceId::new(plan.focus_resource_id.clone())
            .map_err(|error| QuerySourceError::Contract(error.to_string()))?;
        let result = self
            .store
            .read(|store| {
                build_topology(store, &focus_id, plan, &context)
                    .map_err(|error| StoreError::Contract(error.to_string()))
            })
            .map_err(QuerySourceError::from)?;
        let (projection, body) = result;
        Ok(SourceSnapshot {
            metadata: self.metadata(projection, &context),
            body,
        })
    }

    fn get_health_summary(&self) -> Result<SourceSnapshot<HealthSummaryBody>, Self::Error> {
        let context = self.context_snapshot()?;
        let cutoffs = self.cutoffs(&context)?;
        let projection = self
            .store
            .read(|store| store.query_health_summary(&cutoffs))
            .map_err(QuerySourceError::from)?;
        Ok(SourceSnapshot {
            metadata: self.metadata(projection.metadata, &context),
            body: health_summary_dto(projection.body)?,
        })
    }

    fn list_connections(&self) -> Result<SourceSnapshot<Vec<ConnectionDto>>, Self::Error> {
        let context = self.context_snapshot()?;
        let projection = self
            .store
            .read(|store| store.query_connections())
            .map_err(QuerySourceError::from)?;
        let body = projection
            .body
            .into_iter()
            .map(connection_dto)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(SourceSnapshot {
            metadata: self.metadata(projection.metadata, &context),
            body,
        })
    }

    fn get_recent_changes(
        &self,
        plan: &RecentChangesPlan,
    ) -> Result<SourceSnapshot<SourcePage<next_infra_query::dto::ChangeDto>>, Self::Error> {
        let context = self.context_snapshot()?;
        let since_millis = plan.since.as_deref().map(parse_timestamp).transpose()?;
        let resource_id = plan
            .resource_id
            .as_deref()
            .map(|value| {
                ResourceId::new(value.to_owned())
                    .map_err(|error| QuerySourceError::Contract(error.to_string()))
            })
            .transpose()?;
        let projection = self
            .store
            .read(|store| {
                store.query_recent_changes(&RecentChangesProjectionPlan {
                    since_millis,
                    resource_id,
                    kinds: plan.kinds.clone(),
                    limit: plan.limit,
                    after: plan.after.clone(),
                })
            })
            .map_err(QuerySourceError::from)?;
        let body = SourcePage {
            items: projection
                .body
                .items
                .iter()
                .map(change_dto)
                .collect::<Result<Vec<_>, _>>()?,
            next_after: projection.body.next_after,
        };
        Ok(SourceSnapshot {
            metadata: self.metadata(projection.metadata, &context),
            body,
        })
    }

    fn get_timeline(
        &self,
        plan: &TimelinePlan,
    ) -> Result<SourceSnapshot<TimelineSourcePage>, Self::Error> {
        let context = self.context_snapshot()?;
        let projection = self
            .store
            .read(|store| store.query_timeline(plan.limit, plan.after.clone()))
            .map_err(QuerySourceError::from)?;
        let mut groups = Vec::<TimelineGroupDto>::new();
        for item in projection.body.items {
            let origin = timeline_origin_dto(&item.change.origin);
            let occurred_at = format_timestamp(item.change.observed_at);
            let group_id = timeline_group_id(&item.change.origin, item.change.observed_at);
            let item = TimelineItemDto {
                change: change_dto(&item.change)?,
                version_links: item
                    .version_links
                    .into_iter()
                    .map(timeline_version_link_dto)
                    .collect(),
            };
            if groups
                .last()
                .is_some_and(|group| group.group_id == group_id)
            {
                groups
                    .last_mut()
                    .expect("timeline group must exist")
                    .items
                    .push(item);
            } else {
                groups.push(TimelineGroupDto {
                    group_id,
                    origin,
                    occurred_at,
                    items: vec![item],
                });
            }
        }
        Ok(SourceSnapshot {
            metadata: self.metadata(projection.metadata, &context),
            body: TimelineSourcePage {
                item_count: groups.iter().map(|group| group.items.len()).sum(),
                groups,
                next_after: projection.body.next_after,
            },
        })
    }

    fn get_sync_status(
        &self,
        connection_id: &str,
        recent_run_limit: usize,
    ) -> Result<SourceSnapshot<Option<SyncStatusBody>>, Self::Error> {
        let context = self.context_snapshot()?;
        let id = next_infra_core::ConnectionId::new(connection_id.to_owned())
            .map_err(|error| QuerySourceError::Contract(error.to_string()))?;
        let projection = self
            .store
            .read(|store| store.query_sync_status(&id, recent_run_limit))
            .map_err(QuerySourceError::from)?;
        let schedule = projection
            .body
            .as_ref()
            .map(|_| {
                context
                    .schedule(&id)
                    .ok_or_else(|| QuerySourceError::Contract("query schedule is missing".into()))
            })
            .transpose()?;
        let body = projection
            .body
            .map(|status| sync_status_body(status, schedule))
            .transpose()?;
        Ok(SourceSnapshot {
            metadata: self.metadata(projection.metadata, &context),
            body,
        })
    }

    fn list_connector_coverage(
        &self,
    ) -> Result<SourceSnapshot<Vec<ConnectorCoverageDto>>, Self::Error> {
        let context = self.context_snapshot()?;
        // The connection projection gives us committed metadata in the same
        // Store read transaction used for this endpoint. Coverage itself is a
        // descriptor snapshot, not a persisted Store row.
        let projection = self
            .store
            .read(|store| store.query_connections())
            .map_err(QuerySourceError::from)?;
        Ok(SourceSnapshot {
            metadata: self.metadata(projection.metadata, &context),
            body: self.catalog.coverage(None),
        })
    }
}

fn build_topology(
    store: &next_infra_store::Store,
    focus_id: &ResourceId,
    plan: &TopologyPlan,
    context: &QueryContextSnapshot,
) -> Result<(ProjectionMetadata, Option<TopologyBody>), QuerySourceError> {
    let focus_projection = store
        .query_resources_by_ids(&BTreeSet::from([focus_id.clone()]))
        .map_err(QuerySourceError::from)?;
    let metadata = focus_projection.metadata;
    if focus_projection.body.is_empty() {
        return Ok((metadata, None));
    }

    let mut visited = BTreeSet::from([focus_id.clone()]);
    let mut frontier_ids = BTreeSet::from([focus_id.clone()]);
    let mut levels = 0_u8;
    let mut projected_edges = BTreeMap::<String, ProjectedRelation>::new();
    let mut frontier = BTreeSet::<(String, u8)>::new();
    let mut truncated = false;

    while levels < plan.depth && !frontier_ids.is_empty() {
        let remaining_edges = plan.max_edges.saturating_sub(projected_edges.len());
        if remaining_edges == 0 {
            truncated = true;
            break;
        }
        let relation_projection = store
            .query_relations_for_resources(&frontier_ids, remaining_edges.saturating_add(1), None)
            .map_err(QuerySourceError::from)?;
        if relation_projection.metadata != metadata {
            return Err(QuerySourceError::Contract(
                "topology projections changed during query".into(),
            ));
        }
        if relation_projection.body.next_after.is_some() {
            truncated = true;
        }

        let mut next_frontier = BTreeSet::new();
        for projected in relation_projection.body.items {
            let relation = projected.relation.clone();
            let relation_id = relation.relation_id.as_str().to_owned();
            if projected_edges.contains_key(&relation_id) {
                continue;
            }
            let (neighbor, direction) = if frontier_ids.contains(&relation.source_resource_id) {
                (&relation.target_resource_id, FrontierDirectionDto::Outgoing)
            } else if frontier_ids.contains(&relation.target_resource_id) {
                (&relation.source_resource_id, FrontierDirectionDto::Incoming)
            } else {
                continue;
            };

            let neighbor_is_visited = visited.contains(neighbor);
            if !neighbor_is_visited && visited.len() >= plan.max_nodes {
                truncated = true;
                frontier.insert((
                    neighbor.as_str().to_owned(),
                    frontier_direction_key(direction),
                ));
                continue;
            }

            if projected_edges.len() >= plan.max_edges {
                truncated = true;
                add_frontier_for_relation(&mut frontier, &relation, &visited);
                continue;
            }
            projected_edges.insert(relation_id, projected);

            if !neighbor_is_visited {
                visited.insert(neighbor.clone());
                next_frontier.insert(neighbor.clone());
            }
        }
        frontier_ids = next_frontier;
        levels = levels.saturating_add(1);
    }

    let resources_projection = store
        .query_resources_by_ids(&visited)
        .map_err(QuerySourceError::from)?;
    if resources_projection.metadata != metadata {
        return Err(QuerySourceError::Contract(
            "topology projections changed during query".into(),
        ));
    }
    let nodes = resources_projection
        .body
        .iter()
        .map(|resource| resource_dto_from_context(resource, context))
        .collect::<Result<Vec<_>, _>>()?;
    let edges = projected_edges
        .into_values()
        .map(|projected| projected_relation_dto(&projected))
        .collect::<Result<Vec<_>, _>>()?;
    let frontier = frontier
        .into_iter()
        .map(|(resource_id, direction)| TopologyFrontierDto {
            resource_id,
            direction: frontier_direction(direction),
        })
        .collect();
    Ok((
        metadata,
        Some(TopologyBody {
            nodes,
            edges,
            frontier,
            truncated,
        }),
    ))
}

fn add_frontier_for_relation(
    frontier: &mut BTreeSet<(String, u8)>,
    relation: &next_infra_core::Relation,
    visited: &BTreeSet<ResourceId>,
) {
    if visited.contains(&relation.source_resource_id)
        && !visited.contains(&relation.target_resource_id)
    {
        frontier.insert((
            relation.target_resource_id.as_str().to_owned(),
            frontier_direction_key(FrontierDirectionDto::Outgoing),
        ));
    } else if visited.contains(&relation.target_resource_id)
        && !visited.contains(&relation.source_resource_id)
    {
        frontier.insert((
            relation.source_resource_id.as_str().to_owned(),
            frontier_direction_key(FrontierDirectionDto::Incoming),
        ));
    }
}

fn frontier_direction_key(direction: FrontierDirectionDto) -> u8 {
    match direction {
        FrontierDirectionDto::Incoming => 0,
        FrontierDirectionDto::Outgoing => 1,
    }
}

fn frontier_direction(value: u8) -> FrontierDirectionDto {
    if value == 0 {
        FrontierDirectionDto::Incoming
    } else {
        FrontierDirectionDto::Outgoing
    }
}

fn resource_detail_body(
    detail: ResourceDetailProjection,
    include: &BTreeSet<ResourceInclude>,
    context: &QueryContextSnapshot,
    connector_type: Option<&ConnectorType>,
    catalog: &ConnectorCatalogSnapshot,
) -> Result<ResourceDetailBody, QuerySourceError> {
    if include.contains(&ResourceInclude::Relations) && detail.relations_truncated {
        return Err(QuerySourceError::Contract(
            "resource relation projection was truncated".into(),
        ));
    }
    if include.contains(&ResourceInclude::RecentChanges) && detail.recent_changes_truncated {
        return Err(QuerySourceError::Contract(
            "resource change projection was truncated".into(),
        ));
    }
    let resource = resource_dto_from_context(&detail.resource, context)?;
    let attributes = if include.contains(&ResourceInclude::Attributes) {
        detail.resource.attributes.clone()
    } else {
        Default::default()
    };
    let relations = if include.contains(&ResourceInclude::Relations) {
        detail
            .relations
            .iter()
            .map(projected_relation_dto)
            .collect::<Result<Vec<_>, _>>()?
    } else {
        Vec::new()
    };
    let recent_changes = if include.contains(&ResourceInclude::RecentChanges) {
        detail
            .recent_changes
            .iter()
            .map(change_dto)
            .collect::<Result<Vec<_>, _>>()?
    } else {
        Vec::new()
    };
    let connector_coverage = if include.contains(&ResourceInclude::ConnectorCoverage) {
        catalog.coverage(connector_type)
    } else {
        Vec::new()
    };
    Ok(ResourceDetailBody {
        resource,
        attributes,
        relations,
        recent_changes,
        connector_coverage,
    })
}

fn resource_dto(
    source: &Resource,
    freshness: CoreFreshness,
) -> Result<ResourceDto, QuerySourceError> {
    Ok(ResourceDto {
        resource_id: source.resource_id.as_str().to_owned(),
        connection_id: source.connection_id.as_str().to_owned(),
        kind: source.kind.as_str().to_owned(),
        display_name: source.display_name.clone(),
        scope: source.scope.as_str().to_owned(),
        lifecycle: lifecycle(source.lifecycle),
        health: resource_health(source.health),
        freshness: query_freshness(freshness),
        observed_at: format_timestamp(source.last_seen_at),
    })
}

fn resource_dto_from_context(
    source: &Resource,
    context: &QueryContextSnapshot,
) -> Result<ResourceDto, QuerySourceError> {
    let freshness = classify_freshness(source, context)?;
    resource_dto(source, freshness)
}

fn classify_freshness(
    resource: &Resource,
    context: &QueryContextSnapshot,
) -> Result<CoreFreshness, QuerySourceError> {
    let schedule = context
        .schedule(&resource.connection_id)
        .ok_or_else(|| QuerySourceError::Contract("freshness schedule is missing".into()))?;
    let interval = i128::from(schedule.interval_millis);
    if interval == 0 {
        return Err(QuerySourceError::Context(
            QueryContextError::InvalidInterval,
        ));
    }
    let age = i128::from(context.evaluated_at.unix_millis())
        - i128::from(resource.last_seen_at.unix_millis());
    let age = age.max(0);
    if age <= interval {
        Ok(CoreFreshness::Fresh)
    } else if age <= interval.saturating_mul(3) {
        Ok(CoreFreshness::Stale)
    } else {
        Ok(CoreFreshness::Expired)
    }
}

fn projected_relation_dto(projected: &ProjectedRelation) -> Result<RelationDto, QuerySourceError> {
    let relation = &projected.relation;
    let (evidence_type, evidence) = match &relation.evidence {
        RelationEvidence::Provider {
            connection_id,
            sync_run_id,
            field_path,
        } => {
            let connector_type = projected.provider_connector_type.as_ref().ok_or_else(|| {
                QuerySourceError::Contract("provider connector type is missing".into())
            })?;
            (
                EvidenceType::Provider,
                RelationEvidenceDto::Provider {
                    connector_type: connector_type.as_str().to_owned(),
                    connection_id: connection_id.as_str().to_owned(),
                    sync_run_id: sync_run_id.as_str().to_owned(),
                    field_path: field_path.as_str().to_owned(),
                },
            )
        }
        RelationEvidence::Configured { binding_id } => {
            let created_at = projected.configured_created_at.ok_or_else(|| {
                QuerySourceError::Contract("configured relation creation time is missing".into())
            })?;
            (
                EvidenceType::Configured,
                RelationEvidenceDto::Configured {
                    binding_id: binding_id.as_str().to_owned(),
                    created_at: format_timestamp(created_at),
                },
            )
        }
        RelationEvidence::Inferred {
            rule_version,
            input_resource_version_ids,
            input_relation_version_ids,
            confidence,
        } => (
            EvidenceType::Inferred,
            RelationEvidenceDto::Inferred {
                rule_version: rule_version.as_str().to_owned(),
                input_resource_version_ids: input_resource_version_ids
                    .iter()
                    .map(|id| id.as_str().to_owned())
                    .collect(),
                input_relation_version_ids: input_relation_version_ids
                    .iter()
                    .map(|id| id.as_str().to_owned())
                    .collect(),
                confidence_basis_points: confidence.basis_points(),
            },
        ),
    };
    Ok(RelationDto {
        relation_id: relation.relation_id.as_str().to_owned(),
        source_resource_id: relation.source_resource_id.as_str().to_owned(),
        target_resource_id: relation.target_resource_id.as_str().to_owned(),
        kind: relation.kind.as_str().to_owned(),
        lifecycle: lifecycle(relation.lifecycle),
        evidence_type,
        evidence,
        last_seen_at: format_timestamp(relation.last_seen_at),
    })
}

fn connection_dto(source: ProjectedConnection) -> Result<ConnectionDto, QuerySourceError> {
    Ok(ConnectionDto {
        connection_id: source.connection_id.as_str().to_owned(),
        connector_type: source.connector_type.as_str().to_owned(),
        display_name: source.display_name,
        enabled: source.enabled,
        health: connector_health(source.health),
        last_success_at: source.last_success_at.map(format_timestamp),
        last_attempt_at: source.last_attempt_at.map(format_timestamp),
    })
}

fn sync_status_body(
    source: SyncStatusProjection,
    schedule: Option<&QuerySchedule>,
) -> Result<SyncStatusBody, QuerySourceError> {
    let next_scheduled_at = schedule
        .and_then(|schedule| schedule.next_scheduled_at)
        .map(format_timestamp);
    Ok(SyncStatusBody {
        connection: connection_dto(source.connection)?,
        recent_runs: source
            .recent_runs
            .iter()
            .map(sync_run_dto)
            .collect::<Result<Vec<_>, _>>()?,
        next_scheduled_at,
    })
}

fn health_summary_dto(source: HealthProjection) -> Result<HealthSummaryBody, QuerySourceError> {
    let mut resource_health = ResourceHealthCountsDto::default();
    for (health, count) in source.resource_health {
        match health {
            CoreResourceHealth::Healthy => resource_health.healthy = count,
            CoreResourceHealth::Degraded => resource_health.degraded = count,
            CoreResourceHealth::Unhealthy => resource_health.unhealthy = count,
            CoreResourceHealth::Unknown => resource_health.unknown = count,
        }
    }
    let mut freshness = FreshnessCountsDto::default();
    for (value, count) in source.freshness {
        match value {
            CoreFreshness::Fresh => freshness.fresh = count,
            CoreFreshness::Stale => freshness.stale = count,
            CoreFreshness::Expired => freshness.expired = count,
        }
    }
    let mut connector_health = ConnectorHealthCountsDto::default();
    for (health, count) in source.connector_health {
        match health {
            CoreConnectorHealth::Healthy => connector_health.healthy = count,
            CoreConnectorHealth::Degraded => connector_health.degraded = count,
            CoreConnectorHealth::AuthFailed => connector_health.auth_failed = count,
            CoreConnectorHealth::RateLimited => connector_health.rate_limited = count,
            CoreConnectorHealth::Unreachable => connector_health.unreachable = count,
            CoreConnectorHealth::Disabled => connector_health.disabled = count,
        }
    }
    Ok(HealthSummaryBody {
        resource_health,
        freshness,
        connector_health,
    })
}

fn change_dto(change: &Change) -> Result<next_infra_query::dto::ChangeDto, QuerySourceError> {
    let subject = match &change.subject {
        ChangeSubject::Resource { resource_id } => ChangeSubjectDto::Resource {
            resource_id: resource_id.as_str().to_owned(),
        },
        ChangeSubject::Relation { relation_id } => ChangeSubjectDto::Relation {
            relation_id: relation_id.as_str().to_owned(),
        },
        ChangeSubject::Binding { binding_id } => ChangeSubjectDto::Binding {
            binding_id: binding_id.as_str().to_owned(),
        },
    };
    let origin = match &change.origin {
        OriginRef::SyncRun { sync_run_id } => ChangeOriginDto::SyncRun {
            sync_run_id: sync_run_id.as_str().to_owned(),
        },
        OriginRef::Binding { binding_id } => ChangeOriginDto::Binding {
            binding_id: binding_id.as_str().to_owned(),
        },
        OriginRef::Inference {
            rule_version,
            input_resource_version_ids,
            input_relation_version_ids,
        } => ChangeOriginDto::Inference {
            rule_version: rule_version.as_str().to_owned(),
            input_resource_version_ids: input_resource_version_ids
                .iter()
                .map(|id| id.as_str().to_owned())
                .collect(),
            input_relation_version_ids: input_relation_version_ids
                .iter()
                .map(|id| id.as_str().to_owned())
                .collect(),
        },
    };
    Ok(next_infra_query::dto::ChangeDto {
        change_id: change.change_id.as_str().to_owned(),
        subject,
        observed_at: format_timestamp(change.observed_at),
        fields: change
            .fields
            .iter()
            .map(|field| FieldChangeDto {
                path: field.path.as_str().to_owned(),
                before: field.before.clone(),
                after: field.after.clone(),
            })
            .collect(),
        origin,
    })
}

fn timeline_origin_dto(origin: &OriginRef) -> TimelineOriginDto {
    match origin {
        OriginRef::SyncRun { sync_run_id } => TimelineOriginDto::SyncRun {
            sync_run_id: sync_run_id.as_str().to_owned(),
        },
        OriginRef::Binding { binding_id } => TimelineOriginDto::Binding {
            binding_id: binding_id.as_str().to_owned(),
        },
        OriginRef::Inference {
            rule_version,
            input_resource_version_ids,
            input_relation_version_ids,
        } => TimelineOriginDto::Inference {
            rule_version: rule_version.as_str().to_owned(),
            input_resource_version_ids: input_resource_version_ids
                .iter()
                .map(|id| id.as_str().to_owned())
                .collect(),
            input_relation_version_ids: input_relation_version_ids
                .iter()
                .map(|id| id.as_str().to_owned())
                .collect(),
        },
    }
}

fn timeline_group_id(origin: &OriginRef, observed_at: Timestamp) -> String {
    let prefix = match origin {
        OriginRef::SyncRun { sync_run_id } => format!("sync_run:{}", sync_run_id.as_str()),
        OriginRef::Binding { binding_id } => format!("binding:{}", binding_id.as_str()),
        OriginRef::Inference {
            rule_version,
            input_resource_version_ids,
            input_relation_version_ids,
        } => format!(
            "inference:{}:resources={}:relations={}",
            rule_version.as_str(),
            input_resource_version_ids
                .iter()
                .map(|id| id.as_str())
                .collect::<Vec<_>>()
                .join(","),
            input_relation_version_ids
                .iter()
                .map(|id| id.as_str())
                .collect::<Vec<_>>()
                .join(","),
        ),
    };
    format!("timeline:{prefix}:{}", observed_at.unix_millis())
}

fn timeline_version_link_dto(link: TimelineVersionLinkProjection) -> TimelineVersionLinkDto {
    match link {
        TimelineVersionLinkProjection::Resource {
            resource_id,
            resource_version_id,
        } => TimelineVersionLinkDto::Resource {
            resource_id: resource_id.as_str().to_owned(),
            resource_version_id: resource_version_id.as_str().to_owned(),
        },
        TimelineVersionLinkProjection::Relation {
            relation_id,
            relation_version_id,
        } => TimelineVersionLinkDto::Relation {
            relation_id: relation_id.as_str().to_owned(),
            relation_version_id: relation_version_id.as_str().to_owned(),
        },
    }
}

fn sync_run_dto(run: &SyncRun) -> Result<SyncRunDto, QuerySourceError> {
    Ok(SyncRunDto {
        sync_run_id: run.sync_run_id.as_str().to_owned(),
        connection_id: run.connection_id.as_str().to_owned(),
        mode: sync_mode(run.mode),
        trigger: sync_trigger(run.trigger),
        status: sync_status(run.status),
        coverage: sync_coverage(&run.coverage),
        started_at: format_timestamp(run.started_at),
        finished_at: run.finished_at.map(format_timestamp),
        cursor_before: run
            .cursor_before
            .as_ref()
            .map(|cursor| cursor.as_str().to_owned()),
        cursor_after: run
            .cursor_after
            .as_ref()
            .map(|cursor| cursor.as_str().to_owned()),
        counts: SyncRunCountsDto {
            read: run.counts.read,
            created: run.counts.created,
            updated: run.counts.updated,
            unchanged: run.counts.unchanged,
            warnings: run.counts.warnings,
        },
        errors: run
            .errors
            .iter()
            .map(|error| SyncRunErrorDto {
                code: domain_error_code(error),
                message: domain_error_message(error),
                retryable: error.retryable,
            })
            .collect(),
    })
}

fn sync_coverage(coverage: &SyncCoverage) -> SyncCoverageDto {
    match coverage {
        SyncCoverage::AuthoritativeFull { scope } => SyncCoverageDto::AuthoritativeFull {
            scope: scope.as_str().to_owned(),
        },
        SyncCoverage::Incremental { cursor } => SyncCoverageDto::Incremental {
            cursor: cursor.as_str().to_owned(),
        },
        SyncCoverage::Partial { scope, reason } => SyncCoverageDto::Partial {
            scope: scope.as_ref().map(|scope| scope.as_str().to_owned()),
            reason: coverage_gap_reason(reason),
        },
        SyncCoverage::Targeted { resource_ids } => SyncCoverageDto::Targeted {
            resource_ids: resource_ids
                .iter()
                .map(|id| id.as_str().to_owned())
                .collect(),
        },
    }
}

fn coverage_gap_reason(reason: &CoverageGapReason) -> String {
    match reason {
        CoverageGapReason::PermissionDenied => "permission_denied".into(),
        CoverageGapReason::PaginationIncomplete => "pagination_incomplete".into(),
        CoverageGapReason::RateLimited => "rate_limited".into(),
        CoverageGapReason::ProviderUnavailable => "provider_unavailable".into(),
        CoverageGapReason::SchemaIncompatible => "schema_incompatible".into(),
        CoverageGapReason::Other(reason) => reason.clone(),
    }
}

fn domain_error_code(error: &DomainError) -> String {
    format!("{:?}", error.code)
        .chars()
        .enumerate()
        .flat_map(|(index, character)| {
            if character.is_ascii_uppercase() && index != 0 {
                vec!['_', character.to_ascii_lowercase()]
            } else {
                vec![character.to_ascii_lowercase()]
            }
        })
        .collect()
}

fn domain_error_message(error: &DomainError) -> String {
    use next_infra_core::ErrorCode;

    match error.code {
        ErrorCode::InvalidDomainValue => "A synchronized value was invalid.",
        ErrorCode::NotFound => "The requested provider object was not found.",
        ErrorCode::Conflict => "The provider reported a conflicting state.",
        ErrorCode::AuthenticationFailed => "Provider authentication failed.",
        ErrorCode::CredentialUnavailable => "The provider credential was unavailable.",
        ErrorCode::PermissionDenied => "The provider denied the requested read access.",
        ErrorCode::RateLimited => "The provider rate-limited the synchronization.",
        ErrorCode::NetworkUnreachable => "The provider network endpoint was unreachable.",
        ErrorCode::HostKeyMismatch => "The remote host identity did not match.",
        ErrorCode::ProviderUnavailable => "The provider was unavailable.",
        ErrorCode::InvalidResponse => "The provider returned an invalid response.",
        ErrorCode::SchemaIncompatible => "The provider response schema was incompatible.",
        ErrorCode::PartialPagination => "The provider result was only partially paginated.",
        ErrorCode::Cancelled => "The synchronization was cancelled.",
        ErrorCode::Internal => "The synchronization failed internally.",
    }
    .into()
}

fn lifecycle(value: Lifecycle) -> QueryLifecycle {
    match value {
        Lifecycle::Active => QueryLifecycle::Active,
        Lifecycle::Tombstoned => QueryLifecycle::Tombstoned,
        Lifecycle::Orphaned => QueryLifecycle::Orphaned,
    }
}

fn resource_health(value: CoreResourceHealth) -> QueryResourceHealth {
    match value {
        CoreResourceHealth::Healthy => QueryResourceHealth::Healthy,
        CoreResourceHealth::Degraded => QueryResourceHealth::Degraded,
        CoreResourceHealth::Unhealthy => QueryResourceHealth::Unhealthy,
        CoreResourceHealth::Unknown => QueryResourceHealth::Unknown,
    }
}

fn core_resource_health(value: QueryResourceHealth) -> CoreResourceHealth {
    match value {
        QueryResourceHealth::Healthy => CoreResourceHealth::Healthy,
        QueryResourceHealth::Degraded => CoreResourceHealth::Degraded,
        QueryResourceHealth::Unhealthy => CoreResourceHealth::Unhealthy,
        QueryResourceHealth::Unknown => CoreResourceHealth::Unknown,
    }
}

fn query_freshness(value: CoreFreshness) -> QueryFreshness {
    match value {
        CoreFreshness::Fresh => QueryFreshness::Fresh,
        CoreFreshness::Stale => QueryFreshness::Stale,
        CoreFreshness::Expired => QueryFreshness::Expired,
    }
}

fn core_freshness(value: QueryFreshness) -> CoreFreshness {
    match value {
        QueryFreshness::Fresh => CoreFreshness::Fresh,
        QueryFreshness::Stale => CoreFreshness::Stale,
        QueryFreshness::Expired => CoreFreshness::Expired,
    }
}

fn connector_health(value: CoreConnectorHealth) -> QueryConnectorHealth {
    match value {
        CoreConnectorHealth::Healthy => QueryConnectorHealth::Healthy,
        CoreConnectorHealth::Degraded => QueryConnectorHealth::Degraded,
        CoreConnectorHealth::AuthFailed => QueryConnectorHealth::AuthFailed,
        CoreConnectorHealth::RateLimited => QueryConnectorHealth::RateLimited,
        CoreConnectorHealth::Unreachable => QueryConnectorHealth::Unreachable,
        CoreConnectorHealth::Disabled => QueryConnectorHealth::Disabled,
    }
}

fn sync_mode(value: SyncMode) -> SyncModeDto {
    match value {
        SyncMode::Full => SyncModeDto::Full,
        SyncMode::Incremental => SyncModeDto::Incremental,
        SyncMode::Targeted => SyncModeDto::Targeted,
    }
}

fn sync_trigger(value: SyncTrigger) -> SyncTriggerDto {
    match value {
        SyncTrigger::Schedule => SyncTriggerDto::Schedule,
        SyncTrigger::User => SyncTriggerDto::User,
        SyncTrigger::Startup => SyncTriggerDto::Startup,
        SyncTrigger::Recovery => SyncTriggerDto::Recovery,
    }
}

fn sync_status(value: SyncRunStatus) -> SyncRunStatusDto {
    match value {
        SyncRunStatus::Running => SyncRunStatusDto::Running,
        SyncRunStatus::Succeeded => SyncRunStatusDto::Succeeded,
        SyncRunStatus::Partial => SyncRunStatusDto::Partial,
        SyncRunStatus::Failed => SyncRunStatusDto::Failed,
        SyncRunStatus::Cancelled => SyncRunStatusDto::Cancelled,
        SyncRunStatus::Interrupted => SyncRunStatusDto::Interrupted,
    }
}

fn coverage_level(value: ConnectorCoverageLevel) -> ConnectorCoverageLevelDto {
    match value {
        ConnectorCoverageLevel::Supported => ConnectorCoverageLevelDto::Supported,
        ConnectorCoverageLevel::Partial => ConnectorCoverageLevelDto::Partial,
        ConnectorCoverageLevel::Unsupported => ConnectorCoverageLevelDto::Unsupported,
    }
}

pub fn format_timestamp(timestamp: Timestamp) -> String {
    let millis = timestamp.unix_millis();
    let seconds = millis.div_euclid(1_000);
    let milliseconds = millis.rem_euclid(1_000);
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{milliseconds:03}Z")
}

fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 }.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096).div_euclid(365);
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let month_part = (5 * doy + 2).div_euclid(153);
    let day = doy - (153 * month_part + 2).div_euclid(5) + 1;
    let month = month_part + if month_part < 10 { 3 } else { -9 };
    let year = year + if month <= 2 { 1 } else { 0 };
    (year, month, day)
}

fn parse_timestamp(value: &str) -> Result<i64, QuerySourceError> {
    if let Ok(millis) = value.parse::<i64>() {
        return if millis >= 0 {
            Ok(millis)
        } else {
            Err(QuerySourceError::Contract("timestamp is invalid".into()))
        };
    }
    let value = value
        .strip_suffix('Z')
        .ok_or_else(|| QuerySourceError::Contract("timestamp must be UTC".into()))?;
    let (date, time) = value
        .split_once('T')
        .ok_or_else(|| QuerySourceError::Contract("timestamp is invalid".into()))?;
    let mut date_parts = date.split('-');
    let year = date_parts
        .next()
        .and_then(|part| part.parse::<i64>().ok())
        .ok_or_else(|| QuerySourceError::Contract("timestamp is invalid".into()))?;
    let month = date_parts
        .next()
        .and_then(|part| part.parse::<i64>().ok())
        .ok_or_else(|| QuerySourceError::Contract("timestamp is invalid".into()))?;
    let day = date_parts
        .next()
        .and_then(|part| part.parse::<i64>().ok())
        .ok_or_else(|| QuerySourceError::Contract("timestamp is invalid".into()))?;
    if date_parts.next().is_some() || !(1..=12).contains(&month) || day == 0 {
        return Err(QuerySourceError::Contract("timestamp is invalid".into()));
    }
    let (clock, fraction) = time.split_once('.').map_or((time, ""), |parts| parts);
    let mut clock_parts = clock.split(':');
    let hour = clock_parts
        .next()
        .and_then(|part| part.parse::<i64>().ok())
        .ok_or_else(|| QuerySourceError::Contract("timestamp is invalid".into()))?;
    let minute = clock_parts
        .next()
        .and_then(|part| part.parse::<i64>().ok())
        .ok_or_else(|| QuerySourceError::Contract("timestamp is invalid".into()))?;
    let second = clock_parts
        .next()
        .and_then(|part| part.parse::<i64>().ok())
        .ok_or_else(|| QuerySourceError::Contract("timestamp is invalid".into()))?;
    if clock_parts.next().is_some() || hour > 23 || minute > 59 || second > 59 {
        return Err(QuerySourceError::Contract("timestamp is invalid".into()));
    }
    let fraction = if fraction.is_empty() {
        0
    } else {
        if fraction.len() > 3 || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(QuerySourceError::Contract("timestamp is invalid".into()));
        }
        let parsed = fraction
            .parse::<i64>()
            .map_err(|_| QuerySourceError::Contract("timestamp is invalid".into()))?;
        parsed * 10_i64.pow((3 - fraction.len()) as u32)
    };
    let days = days_from_civil(year, month, day)
        .ok_or_else(|| QuerySourceError::Contract("timestamp is invalid".into()))?;
    days.checked_mul(86_400_000)
        .and_then(|value| value.checked_add(hour * 3_600_000))
        .and_then(|value| value.checked_add(minute * 60_000))
        .and_then(|value| value.checked_add(second * 1_000))
        .and_then(|value| value.checked_add(fraction))
        .filter(|value| *value >= 0)
        .ok_or_else(|| QuerySourceError::Contract("timestamp is invalid".into()))
}

fn days_from_civil(year: i64, month: i64, day: i64) -> Option<i64> {
    let month_days = [31_i64, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let max_day =
        month_days[usize::try_from(month - 1).ok()?] + if month == 2 && leap { 1 } else { 0 };
    if day < 1 || day > max_day {
        return None;
    }
    let adjusted_year = year - if month <= 2 { 1 } else { 0 };
    let era = (if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    })
    .div_euclid(400);
    let year_of_era = adjusted_year - era * 400;
    let month_part = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month_part + 2).div_euclid(5) + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era.checked_mul(146_097)
        .and_then(|value| value.checked_add(day_of_era))
        .and_then(|value| value.checked_sub(719_468))
}
