use crate::projection::{
    read_change, read_connection, read_relation, read_resource, read_sync_run, wrapped,
};
use crate::{Store, StoreError};
use next_infra_core::{
    Change, ChangeSubject, Connection, ConnectionId, ConnectorHealth, ConnectorType, Freshness,
    OriginRef, Relation, RelationVersionId, Resource, ResourceHealth, ResourceId,
    ResourceVersionId, SyncRun, Timestamp,
};
use rusqlite::types::{Type, Value};
use rusqlite::{Connection as SqliteConnection, OptionalExtension, params, params_from_iter};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_PROJECTION_PAGE_LIMIT: usize = 512;
pub const MAX_DETAIL_RELATIONS: usize = 400;
pub const MAX_DETAIL_CHANGES: usize = 100;

const RESOURCE_COLUMNS: &str = "r.resource_id, r.connection_id, r.kind, r.external_id, r.name, r.display_name, r.scope, r.labels_json, r.lifecycle, r.health, r.attributes_json, r.attribute_schema_version, r.fingerprint, r.first_seen_at, r.last_seen_at, r.last_changed_at, r.last_sync_run_id";
const PROJECTED_RELATION_COLUMNS: &str = "r.relation_id, r.source_resource_id, r.target_resource_id, r.kind, r.evidence_key, r.evidence_json, r.first_seen_at, r.last_seen_at, r.lifecycle";
const CHANGE_COLUMNS: &str =
    "change_id, subject_type, subject_id, observed_at, fields_json, origin_json";
const CONNECTION_COLUMNS: &str = "connection_id, connector_type, display_name, enabled, config_json, secret_ref, health, last_success_at, last_attempt_at, config_schema_version, deleted_at";
const SYNC_RUN_COLUMNS: &str = "sync_run_id, connection_id, mode, trigger, started_at, finished_at, status, coverage_json, cursor_before, cursor_after, counts_json, errors_json, warnings_json";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProjectionMetadata {
    pub committed_revision: u64,
    pub committed_at_millis: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectionSnapshot<T> {
    pub metadata: ProjectionMetadata,
    pub body: T,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectionPage<T> {
    pub items: Vec<T>,
    pub next_after: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FreshnessCutoffs {
    pub fresh_after_millis: i64,
    pub expired_after_millis: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceProjectionPlan {
    pub query: Option<String>,
    pub kinds: BTreeSet<String>,
    pub connector_types: BTreeSet<String>,
    pub health: Vec<ResourceHealth>,
    pub freshness: Vec<Freshness>,
    pub labels: BTreeMap<String, String>,
    pub cutoffs: BTreeMap<ConnectionId, FreshnessCutoffs>,
    pub limit: usize,
    pub after: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectedResource {
    pub resource: Resource,
    pub connector_type: ConnectorType,
    pub freshness: Freshness,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectedRelation {
    pub relation: Relation,
    pub provider_connector_type: Option<ConnectorType>,
    pub configured_created_at: Option<Timestamp>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceDetailProjection {
    pub resource: Resource,
    pub relations: Vec<ProjectedRelation>,
    pub recent_changes: Vec<Change>,
    pub relations_truncated: bool,
    pub recent_changes_truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectedConnection {
    pub connection_id: ConnectionId,
    pub connector_type: ConnectorType,
    pub display_name: String,
    pub enabled: bool,
    pub health: ConnectorHealth,
    pub last_success_at: Option<next_infra_core::Timestamp>,
    pub last_attempt_at: Option<next_infra_core::Timestamp>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HealthProjection {
    pub resource_health: Vec<(ResourceHealth, u64)>,
    pub freshness: Vec<(Freshness, u64)>,
    pub connector_health: Vec<(ConnectorHealth, u64)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecentChangesProjectionPlan {
    pub since_millis: Option<i64>,
    pub resource_id: Option<ResourceId>,
    pub kinds: BTreeSet<String>,
    pub limit: usize,
    pub after: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TimelineVersionLinkProjection {
    Resource {
        resource_id: ResourceId,
        resource_version_id: ResourceVersionId,
    },
    Relation {
        relation_id: next_infra_core::RelationId,
        relation_version_id: RelationVersionId,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimelineProjectionItem {
    pub change: Change,
    pub version_links: Vec<TimelineVersionLinkProjection>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncStatusProjection {
    pub connection: ProjectedConnection,
    pub recent_runs: Vec<SyncRun>,
}

/// Per-repository GitHub Actions run counts derived from the relations chain:
/// repository →(github.contains)→ workflow →(github.executes)→ workflow_run
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitHubActionsSummaryRow {
    pub connection_id: ConnectionId,
    pub connection_name: String,
    pub repository_id: ResourceId,
    pub repository_name: String,
    pub action_count: u64,
    pub succeeded: u64,
    pub failed: u64,
    pub running: u64,
}

impl Store {
    pub fn query_resources(
        &self,
        plan: &ResourceProjectionPlan,
    ) -> Result<ProjectionSnapshot<ProjectionPage<ProjectedResource>>, StoreError> {
        validate_limit(plan.limit)?;
        self.with_projection_snapshot(|connection| {
            validate_cutoffs(connection, &plan.cutoffs)?;
            read_resource_page(connection, plan)
        })
    }

    pub fn query_resource_detail(
        &self,
        resource_id: &ResourceId,
    ) -> Result<ProjectionSnapshot<Option<ResourceDetailProjection>>, StoreError> {
        self.with_projection_snapshot(|connection| read_resource_detail(connection, resource_id))
    }

    pub fn query_relations_for_resources(
        &self,
        resource_ids: &BTreeSet<ResourceId>,
        limit: usize,
        after: Option<&str>,
    ) -> Result<ProjectionSnapshot<ProjectionPage<ProjectedRelation>>, StoreError> {
        validate_limit(limit)?;
        if resource_ids.len() > MAX_PROJECTION_PAGE_LIMIT {
            return Err(StoreError::Contract(
                "too many topology frontier resources".into(),
            ));
        }
        self.with_projection_snapshot(|connection| {
            read_relations_for_resources(connection, resource_ids, limit, after, false)
        })
    }

    pub fn query_relations_within_resources(
        &self,
        resource_ids: &BTreeSet<ResourceId>,
        limit: usize,
        after: Option<&str>,
    ) -> Result<ProjectionSnapshot<ProjectionPage<ProjectedRelation>>, StoreError> {
        validate_limit(limit)?;
        if resource_ids.len() > MAX_PROJECTION_PAGE_LIMIT {
            return Err(StoreError::Contract("too many resource ids".into()));
        }
        self.with_projection_snapshot(|connection| {
            read_relations_for_resources(connection, resource_ids, limit, after, true)
        })
    }

    pub fn query_resources_by_ids(
        &self,
        resource_ids: &BTreeSet<ResourceId>,
    ) -> Result<ProjectionSnapshot<Vec<Resource>>, StoreError> {
        if resource_ids.len() > MAX_PROJECTION_PAGE_LIMIT {
            return Err(StoreError::Contract("too many resource ids".into()));
        }
        self.with_projection_snapshot(|connection| read_resources_by_ids(connection, resource_ids))
    }

    pub fn query_recent_changes(
        &self,
        plan: &RecentChangesProjectionPlan,
    ) -> Result<ProjectionSnapshot<ProjectionPage<Change>>, StoreError> {
        validate_limit(plan.limit)?;
        self.with_projection_snapshot(|connection| read_recent_changes(connection, plan))
    }

    pub fn query_timeline(
        &self,
        limit: usize,
        after: Option<String>,
    ) -> Result<ProjectionSnapshot<ProjectionPage<TimelineProjectionItem>>, StoreError> {
        validate_limit(limit)?;
        self.with_projection_snapshot(|connection| {
            let page = read_recent_changes(
                connection,
                &RecentChangesProjectionPlan {
                    since_millis: None,
                    resource_id: None,
                    kinds: BTreeSet::new(),
                    limit,
                    after,
                },
            )?;
            let items = page
                .items
                .into_iter()
                .map(|change| {
                    let version_links = read_timeline_version_links(connection, &change)?;
                    Ok(TimelineProjectionItem {
                        change,
                        version_links,
                    })
                })
                .collect::<Result<Vec<_>, StoreError>>()?;
            Ok(ProjectionPage {
                items,
                next_after: page.next_after,
            })
        })
    }

    pub fn query_sync_status(
        &self,
        connection_id: &ConnectionId,
        recent_run_limit: usize,
    ) -> Result<ProjectionSnapshot<Option<SyncStatusProjection>>, StoreError> {
        validate_limit(recent_run_limit)?;
        self.with_projection_snapshot(|connection| {
            read_sync_status(connection, connection_id, recent_run_limit)
        })
    }

    pub fn query_health_summary(
        &self,
        cutoffs: &BTreeMap<ConnectionId, FreshnessCutoffs>,
    ) -> Result<ProjectionSnapshot<HealthProjection>, StoreError> {
        self.with_projection_snapshot(|connection| {
            validate_cutoffs(connection, cutoffs)?;
            read_health_projection(connection, cutoffs)
        })
    }

    pub fn query_connections(
        &self,
    ) -> Result<ProjectionSnapshot<Vec<ProjectedConnection>>, StoreError> {
        self.with_projection_snapshot(read_connections)
    }

    pub fn query_github_actions_summary(
        &self,
    ) -> Result<ProjectionSnapshot<Vec<GitHubActionsSummaryRow>>, StoreError> {
        self.with_projection_snapshot(read_github_actions_summary)
    }

    fn with_projection_snapshot<T>(
        &self,
        read: impl FnOnce(&SqliteConnection) -> Result<T, StoreError>,
    ) -> Result<ProjectionSnapshot<T>, StoreError> {
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(StoreError::Sqlite)?;
        let metadata = read_projection_metadata(&transaction)?;
        let body = read(&transaction)?;
        transaction.commit().map_err(StoreError::Sqlite)?;
        Ok(ProjectionSnapshot { metadata, body })
    }
}

pub(crate) fn read_projection_metadata(
    connection: &SqliteConnection,
) -> Result<ProjectionMetadata, StoreError> {
    let (committed_revision, committed_at_millis): (i64, i64) = connection
        .query_row(
            "SELECT committed_revision, committed_at FROM projection_metadata WHERE singleton_id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(StoreError::Sqlite)?;
    Ok(ProjectionMetadata {
        committed_revision: u64::try_from(committed_revision)
            .map_err(|_| StoreError::Contract("projection revision cannot be negative".into()))?,
        committed_at_millis,
    })
}

fn validate_limit(limit: usize) -> Result<(), StoreError> {
    if limit == 0 || limit > MAX_PROJECTION_PAGE_LIMIT {
        Err(StoreError::Contract(
            "projection limit is outside the supported range".into(),
        ))
    } else {
        Ok(())
    }
}

fn validate_cutoffs(
    connection: &SqliteConnection,
    cutoffs: &BTreeMap<ConnectionId, FreshnessCutoffs>,
) -> Result<(), StoreError> {
    for cutoff in cutoffs.values() {
        if cutoff.expired_after_millis > cutoff.fresh_after_millis {
            return Err(StoreError::Contract(
                "freshness cutoffs are inverted".into(),
            ));
        }
    }
    let mut statement = connection
        .prepare("SELECT DISTINCT connection_id FROM resources ORDER BY connection_id")
        .map_err(StoreError::Sqlite)?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(StoreError::Sqlite)?;
    for row in rows {
        let connection_id: ConnectionId =
            wrapped(row.map_err(StoreError::Sqlite)?).map_err(StoreError::Sqlite)?;
        if !cutoffs.contains_key(&connection_id) {
            return Err(StoreError::Contract(format!(
                "freshness cutoff is missing for connection {}",
                connection_id.as_str()
            )));
        }
    }
    Ok(())
}

fn read_resource_page(
    connection: &SqliteConnection,
    plan: &ResourceProjectionPlan,
) -> Result<ProjectionPage<ProjectedResource>, StoreError> {
    let mut sql = format!(
        "SELECT {RESOURCE_COLUMNS}, c.connector_type FROM resources r JOIN connections c ON c.connection_id = r.connection_id WHERE 1 = 1"
    );
    let mut values = Vec::<Value>::new();

    if let Some(after) = &plan.after {
        sql.push_str(" AND r.resource_id > ?");
        values.push(Value::Text(after.clone()));
    }
    if let Some(query) = &plan.query {
        sql.push_str(" AND (LOWER(r.display_name) LIKE ? ESCAPE '\\' OR LOWER(r.name) LIKE ? ESCAPE '\\' OR LOWER(r.kind) LIKE ? ESCAPE '\\' OR LOWER(r.external_id) LIKE ? ESCAPE '\\' OR LOWER(r.resource_id) LIKE ? ESCAPE '\\')");
        let pattern = format!("%{}%", escape_like(&query.to_lowercase()));
        values.extend((0..5).map(|_| Value::Text(pattern.clone())));
    }
    push_text_set(&mut sql, &mut values, "r.kind", &plan.kinds);
    push_text_set(
        &mut sql,
        &mut values,
        "c.connector_type",
        &plan.connector_types,
    );
    if !plan.health.is_empty() {
        let health = plan
            .health
            .iter()
            .map(|value| resource_health_text(*value).to_owned())
            .collect::<BTreeSet<_>>();
        push_text_set(&mut sql, &mut values, "r.health", &health);
    }
    for (key, value) in &plan.labels {
        sql.push_str(" AND EXISTS (SELECT 1 FROM json_each(r.labels_json) label WHERE label.key = ? AND CAST(label.value AS TEXT) = ?)");
        values.push(Value::Text(key.clone()));
        values.push(Value::Text(value.clone()));
    }
    push_freshness_filter(&mut sql, &mut values, &plan.freshness, &plan.cutoffs);
    sql.push_str(" ORDER BY r.resource_id LIMIT ?");
    values.push(Value::Integer(
        i64::try_from(plan.limit + 1).map_err(|_| StoreError::Contract("limit overflow".into()))?,
    ));

    let mut statement = connection.prepare(&sql).map_err(StoreError::Sqlite)?;
    let rows = statement
        .query_map(params_from_iter(values.iter()), |row| {
            let resource = read_resource(row)?;
            let connector_type = wrapped(row.get(17)?)?;
            Ok((resource, connector_type))
        })
        .map_err(StoreError::Sqlite)?;
    let mut items = Vec::new();
    for row in rows {
        let (resource, connector_type) = row.map_err(StoreError::Sqlite)?;
        let freshness = classify_freshness(&resource, &plan.cutoffs)?;
        items.push(ProjectedResource {
            resource,
            connector_type,
            freshness,
        });
    }
    let truncated = items.len() > plan.limit;
    if truncated {
        items.pop();
    }
    let next_after = truncated
        .then(|| {
            items
                .last()
                .map(|item| item.resource.resource_id.as_str().to_owned())
        })
        .flatten();
    Ok(ProjectionPage { items, next_after })
}

fn read_resource_detail(
    connection: &SqliteConnection,
    resource_id: &ResourceId,
) -> Result<Option<ResourceDetailProjection>, StoreError> {
    let resource = connection
        .query_row(
            &format!("SELECT {RESOURCE_COLUMNS} FROM resources r WHERE r.resource_id = ?1"),
            params![resource_id.as_str()],
            read_resource,
        )
        .optional()
        .map_err(StoreError::Sqlite)?;
    let Some(resource) = resource else {
        return Ok(None);
    };

    let mut relation_statement = connection
        .prepare(&format!(
            "SELECT {PROJECTED_RELATION_COLUMNS}, provider_connection.connector_type, configured_binding.created_at FROM relations r LEFT JOIN connections provider_connection ON r.evidence_type = 'provider' AND json_extract(r.evidence_json, '$.connection_id') = provider_connection.connection_id LEFT JOIN bindings configured_binding ON r.evidence_type = 'configured' AND json_extract(r.evidence_json, '$.binding_id') = configured_binding.binding_id WHERE r.source_resource_id = ?1 OR r.target_resource_id = ?1 ORDER BY r.relation_id LIMIT ?2"
        ))
        .map_err(StoreError::Sqlite)?;
    let relation_rows = relation_statement
        .query_map(
            params![resource_id.as_str(), (MAX_DETAIL_RELATIONS + 1) as i64],
            read_projected_relation,
        )
        .map_err(StoreError::Sqlite)?;
    let mut relations = relation_rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StoreError::Sqlite)?;
    let relations_truncated = relations.len() > MAX_DETAIL_RELATIONS;
    relations.truncate(MAX_DETAIL_RELATIONS);

    let mut change_statement = connection
        .prepare(&format!(
            "SELECT {CHANGE_COLUMNS} FROM changes WHERE (subject_type = 'resource' AND subject_id = ?1) OR (subject_type = 'relation' AND EXISTS (SELECT 1 FROM relations rel WHERE rel.relation_id = changes.subject_id AND (rel.source_resource_id = ?1 OR rel.target_resource_id = ?1))) ORDER BY observed_at DESC, change_id DESC LIMIT ?2"
        ))
        .map_err(StoreError::Sqlite)?;
    let change_rows = change_statement
        .query_map(
            params![resource_id.as_str(), (MAX_DETAIL_CHANGES + 1) as i64],
            read_change,
        )
        .map_err(StoreError::Sqlite)?;
    let mut recent_changes = change_rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StoreError::Sqlite)?;
    let recent_changes_truncated = recent_changes.len() > MAX_DETAIL_CHANGES;
    recent_changes.truncate(MAX_DETAIL_CHANGES);

    Ok(Some(ResourceDetailProjection {
        resource,
        relations,
        recent_changes,
        relations_truncated,
        recent_changes_truncated,
    }))
}

fn read_relations_for_resources(
    connection: &SqliteConnection,
    resource_ids: &BTreeSet<ResourceId>,
    limit: usize,
    after: Option<&str>,
    require_both_endpoints: bool,
) -> Result<ProjectionPage<ProjectedRelation>, StoreError> {
    if resource_ids.is_empty() {
        return Ok(ProjectionPage {
            items: Vec::new(),
            next_after: None,
        });
    }
    let placeholders = placeholders(resource_ids.len());
    let endpoint_predicate = if require_both_endpoints {
        format!(
            "r.source_resource_id IN ({placeholders}) AND r.target_resource_id IN ({placeholders})"
        )
    } else {
        format!(
            "r.source_resource_id IN ({placeholders}) OR r.target_resource_id IN ({placeholders})"
        )
    };
    let mut sql = format!(
        "SELECT {PROJECTED_RELATION_COLUMNS}, provider_connection.connector_type, configured_binding.created_at FROM relations r LEFT JOIN connections provider_connection ON r.evidence_type = 'provider' AND json_extract(r.evidence_json, '$.connection_id') = provider_connection.connection_id LEFT JOIN bindings configured_binding ON r.evidence_type = 'configured' AND json_extract(r.evidence_json, '$.binding_id') = configured_binding.binding_id WHERE ({endpoint_predicate})"
    );
    let mut values = resource_ids
        .iter()
        .map(|id| Value::Text(id.as_str().to_owned()))
        .chain(
            resource_ids
                .iter()
                .map(|id| Value::Text(id.as_str().to_owned())),
        )
        .collect::<Vec<_>>();
    if let Some(after) = after {
        sql.push_str(" AND r.relation_id > ?");
        values.push(Value::Text(after.to_owned()));
    }
    sql.push_str(" ORDER BY r.relation_id LIMIT ?");
    values.push(Value::Integer((limit + 1) as i64));
    let mut statement = connection.prepare(&sql).map_err(StoreError::Sqlite)?;
    let rows = statement
        .query_map(params_from_iter(values.iter()), read_projected_relation)
        .map_err(StoreError::Sqlite)?;
    let mut items = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StoreError::Sqlite)?;
    let truncated = items.len() > limit;
    if truncated {
        items.pop();
    }
    let next_after = truncated
        .then(|| {
            items
                .last()
                .map(|item| item.relation.relation_id.as_str().to_owned())
        })
        .flatten();
    Ok(ProjectionPage { items, next_after })
}

fn read_projected_relation(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProjectedRelation> {
    let relation = read_relation(row)?;
    let provider_connector_type = row.get::<_, Option<String>>(9)?.map(wrapped).transpose()?;
    let configured_created_at = row
        .get::<_, Option<i64>>(10)?
        .map(|value| {
            Timestamp::from_unix_millis(value).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(10, Type::Integer, Box::new(error))
            })
        })
        .transpose()?;
    Ok(ProjectedRelation {
        relation,
        provider_connector_type,
        configured_created_at,
    })
}

fn read_resources_by_ids(
    connection: &SqliteConnection,
    resource_ids: &BTreeSet<ResourceId>,
) -> Result<Vec<Resource>, StoreError> {
    if resource_ids.is_empty() {
        return Ok(Vec::new());
    }
    let sql = format!(
        "SELECT {RESOURCE_COLUMNS} FROM resources r WHERE r.resource_id IN ({}) ORDER BY r.resource_id",
        placeholders(resource_ids.len())
    );
    let values = resource_ids
        .iter()
        .map(|id| Value::Text(id.as_str().to_owned()))
        .collect::<Vec<_>>();
    let mut statement = connection.prepare(&sql).map_err(StoreError::Sqlite)?;
    statement
        .query_map(params_from_iter(values.iter()), read_resource)
        .map_err(StoreError::Sqlite)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(StoreError::Sqlite)
}

fn read_recent_changes(
    connection: &SqliteConnection,
    plan: &RecentChangesProjectionPlan,
) -> Result<ProjectionPage<Change>, StoreError> {
    let mut sql = format!("SELECT {CHANGE_COLUMNS} FROM changes WHERE 1 = 1");
    let mut values = Vec::<Value>::new();
    if let Some(since) = plan.since_millis {
        sql.push_str(" AND observed_at >= ?");
        values.push(Value::Integer(since));
    }
    if let Some(resource_id) = &plan.resource_id {
        sql.push_str(" AND ((subject_type = 'resource' AND subject_id = ?) OR (subject_type = 'relation' AND EXISTS (SELECT 1 FROM relations rel WHERE rel.relation_id = changes.subject_id AND (rel.source_resource_id = ? OR rel.target_resource_id = ?))))");
        values.extend((0..3).map(|_| Value::Text(resource_id.as_str().to_owned())));
    }
    if !plan.kinds.is_empty() {
        let marks = placeholders(plan.kinds.len());
        sql.push_str(&format!(" AND ((subject_type = 'resource' AND EXISTS (SELECT 1 FROM resources res WHERE res.resource_id = changes.subject_id AND res.kind IN ({marks}))) OR (subject_type = 'relation' AND EXISTS (SELECT 1 FROM relations rel WHERE rel.relation_id = changes.subject_id AND rel.kind IN ({marks}))))"));
        values.extend(plan.kinds.iter().cloned().map(Value::Text));
        values.extend(plan.kinds.iter().cloned().map(Value::Text));
    }
    if let Some(after) = &plan.after {
        let (observed_at, change_id) = parse_change_cursor(after)?;
        sql.push_str(" AND (observed_at < ? OR (observed_at = ? AND change_id < ?))");
        values.push(Value::Integer(observed_at));
        values.push(Value::Integer(observed_at));
        values.push(Value::Text(change_id));
    }
    sql.push_str(" ORDER BY observed_at DESC, change_id DESC LIMIT ?");
    values.push(Value::Integer((plan.limit + 1) as i64));
    let mut statement = connection.prepare(&sql).map_err(StoreError::Sqlite)?;
    let rows = statement
        .query_map(params_from_iter(values.iter()), read_change)
        .map_err(StoreError::Sqlite)?;
    let mut items = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StoreError::Sqlite)?;
    let truncated = items.len() > plan.limit;
    if truncated {
        items.pop();
    }
    let next_after = truncated.then(|| items.last().map(change_cursor)).flatten();
    Ok(ProjectionPage { items, next_after })
}

fn read_timeline_version_links(
    connection: &SqliteConnection,
    change: &Change,
) -> Result<Vec<TimelineVersionLinkProjection>, StoreError> {
    match &change.subject {
        ChangeSubject::Resource { resource_id } => {
            let OriginRef::SyncRun { sync_run_id } = &change.origin else {
                return Ok(Vec::new());
            };
            let version_id = connection
                .query_row(
                    "SELECT version_id FROM resource_versions WHERE resource_id = ?1 AND observed_at = ?2 AND sync_run_id = ?3 ORDER BY version_id DESC LIMIT 1",
                    params![resource_id.as_str(), change.observed_at.unix_millis(), sync_run_id.as_str()],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(StoreError::Sqlite)?;
            version_id
                .map(|value| {
                    Ok(TimelineVersionLinkProjection::Resource {
                        resource_id: resource_id.clone(),
                        resource_version_id: wrapped(value).map_err(StoreError::Sqlite)?,
                    })
                })
                .transpose()
                .map(|value| value.into_iter().collect())
        }
        ChangeSubject::Relation { relation_id } => {
            read_relation_version_links(connection, relation_id, change.observed_at, &change.origin)
        }
        ChangeSubject::Binding { .. } => {
            let OriginRef::Binding { .. } = &change.origin else {
                return Ok(Vec::new());
            };
            let mut statement = connection
                .prepare(
                    "SELECT relation_id, relation_version_id FROM relation_versions WHERE observed_at = ?1 AND origin_json = ?2 ORDER BY relation_id, relation_version_id",
                )
                .map_err(StoreError::Sqlite)?;
            let origin = serde_json::to_string(&change.origin).map_err(StoreError::Json)?;
            statement
                .query_map(params![change.observed_at.unix_millis(), origin], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(StoreError::Sqlite)?
                .map(|row| {
                    let (relation_id, relation_version_id) = row.map_err(StoreError::Sqlite)?;
                    Ok(TimelineVersionLinkProjection::Relation {
                        relation_id: wrapped(relation_id).map_err(StoreError::Sqlite)?,
                        relation_version_id: wrapped(relation_version_id)
                            .map_err(StoreError::Sqlite)?,
                    })
                })
                .collect()
        }
    }
}

fn read_relation_version_links(
    connection: &SqliteConnection,
    relation_id: &next_infra_core::RelationId,
    observed_at: Timestamp,
    origin: &OriginRef,
) -> Result<Vec<TimelineVersionLinkProjection>, StoreError> {
    let origin = serde_json::to_string(origin).map_err(StoreError::Json)?;
    let version_id = connection
        .query_row(
            "SELECT relation_version_id FROM relation_versions WHERE relation_id = ?1 AND observed_at = ?2 AND origin_json = ?3 ORDER BY relation_version_id DESC LIMIT 1",
            params![relation_id.as_str(), observed_at.unix_millis(), origin],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(StoreError::Sqlite)?;
    version_id
        .map(|value| {
            Ok(TimelineVersionLinkProjection::Relation {
                relation_id: relation_id.clone(),
                relation_version_id: wrapped(value).map_err(StoreError::Sqlite)?,
            })
        })
        .transpose()
        .map(|value| value.into_iter().collect())
}

fn read_sync_status(
    connection: &SqliteConnection,
    connection_id: &ConnectionId,
    recent_run_limit: usize,
) -> Result<Option<SyncStatusProjection>, StoreError> {
    let connection_row = connection
        .query_row(
            &format!("SELECT {CONNECTION_COLUMNS} FROM connections WHERE connection_id = ?1"),
            params![connection_id.as_str()],
            read_connection,
        )
        .optional()
        .map_err(StoreError::Sqlite)?;
    let Some(connection_row) = connection_row else {
        return Ok(None);
    };
    let mut statement = connection
        .prepare(&format!("SELECT {SYNC_RUN_COLUMNS} FROM sync_runs WHERE connection_id = ?1 ORDER BY started_at DESC, sync_run_id DESC LIMIT ?2"))
        .map_err(StoreError::Sqlite)?;
    let recent_runs = statement
        .query_map(
            params![connection_id.as_str(), recent_run_limit as i64],
            read_sync_run,
        )
        .map_err(StoreError::Sqlite)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(StoreError::Sqlite)?;
    Ok(Some(SyncStatusProjection {
        connection: projected_connection(connection_row),
        recent_runs,
    }))
}

fn read_health_projection(
    connection: &SqliteConnection,
    cutoffs: &BTreeMap<ConnectionId, FreshnessCutoffs>,
) -> Result<HealthProjection, StoreError> {
    let resource_health = grouped_counts(
        connection,
        "SELECT health, COUNT(*) FROM resources GROUP BY health",
    )?
    .into_iter()
    .map(|(value, count)| Ok((resource_health_value(&value)?, count)))
    .collect::<Result<Vec<_>, StoreError>>()?;
    let connector_health = grouped_counts(
        connection,
        "SELECT health, COUNT(*) FROM connections WHERE deleted_at IS NULL GROUP BY health",
    )?
    .into_iter()
    .map(|(value, count)| Ok((connector_health_value(&value)?, count)))
    .collect::<Result<Vec<_>, StoreError>>()?;
    let mut freshness_counts = [0_u64; 3];
    let mut statement = connection
        .prepare("SELECT connection_id, last_seen_at FROM resources")
        .map_err(StoreError::Sqlite)?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(StoreError::Sqlite)?;
    for row in rows {
        let (connection_id, last_seen_at) = row.map_err(StoreError::Sqlite)?;
        let connection_id: ConnectionId = wrapped(connection_id).map_err(StoreError::Sqlite)?;
        let freshness = classify_millis(
            last_seen_at,
            cutoffs.get(&connection_id).ok_or_else(|| {
                StoreError::Contract("freshness cutoff disappeared during query".into())
            })?,
        );
        freshness_counts[freshness_index(freshness)] += 1;
    }
    Ok(HealthProjection {
        resource_health,
        freshness: vec![
            (Freshness::Fresh, freshness_counts[0]),
            (Freshness::Stale, freshness_counts[1]),
            (Freshness::Expired, freshness_counts[2]),
        ],
        connector_health,
    })
}

fn read_connections(connection: &SqliteConnection) -> Result<Vec<ProjectedConnection>, StoreError> {
    let mut statement = connection
        .prepare(&format!("SELECT {CONNECTION_COLUMNS} FROM connections WHERE deleted_at IS NULL ORDER BY connection_id"))
        .map_err(StoreError::Sqlite)?;
    let connections = statement
        .query_map([], read_connection)
        .map_err(StoreError::Sqlite)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(StoreError::Sqlite)?;
    Ok(connections.into_iter().map(projected_connection).collect())
}

fn projected_connection(connection: Connection) -> ProjectedConnection {
    ProjectedConnection {
        connection_id: connection.connection_id,
        connector_type: connection.connector_type,
        display_name: connection.display_name,
        enabled: connection.enabled,
        health: connection.health,
        last_success_at: connection.last_success_at,
        last_attempt_at: connection.last_attempt_at,
    }
}

fn read_github_actions_summary(
    connection: &SqliteConnection,
) -> Result<Vec<GitHubActionsSummaryRow>, StoreError> {
    let github_connections: BTreeMap<ConnectionId, String> = connection
        .prepare(
            "SELECT connection_id, display_name FROM connections
             WHERE connector_type = 'github' AND deleted_at IS NULL
             ORDER BY display_name",
        )
        .map_err(StoreError::Sqlite)?
        .query_map([], |row| Ok((wrapped(row.get(0)?)?, row.get(1)?)))
        .map_err(StoreError::Sqlite)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(StoreError::Sqlite)?
        .into_iter()
        .collect();

    if github_connections.is_empty() {
        return Ok(Vec::new());
    }

    let connection_ids: Vec<String> = github_connections
        .keys()
        .map(|id| id.as_str().to_owned())
        .collect();
    let placeholders = placeholders(connection_ids.len());
    let mut statement = connection
        .prepare(&format!(
            "SELECT r_repo.connection_id, r_repo.resource_id, r_repo.display_name,
                    r_wr.resource_id, r_wr.attributes_json
             FROM resources r_wr
             JOIN relations rel_exec ON rel_exec.target_resource_id = r_wr.resource_id
                 AND rel_exec.kind = 'github.executes'
             JOIN resources r_wf ON r_wf.resource_id = rel_exec.source_resource_id
             JOIN relations rel_contains ON rel_contains.target_resource_id = r_wf.resource_id
                 AND rel_contains.kind = 'github.contains'
             JOIN resources r_repo ON r_repo.resource_id = rel_contains.source_resource_id
             WHERE r_wr.kind = 'github.workflow_run'
               AND r_repo.connection_id IN ({placeholders})
             ORDER BY r_repo.connection_id, r_repo.display_name",
            placeholders = placeholders
        ))
        .map_err(StoreError::Sqlite)?;

    let rows = statement
        .query_map(params_from_iter(connection_ids.iter()), |row| {
            let connection_id_str: String = row.get(0)?;
            let repository_id_str: String = row.get(1)?;
            let repository_name: String = row.get(2)?;
            let workflow_run_id: String = row.get(3)?;
            let attributes_json: String = row.get(4)?;
            Ok((
                connection_id_str,
                repository_id_str,
                repository_name,
                workflow_run_id,
                attributes_json,
            ))
        })
        .map_err(StoreError::Sqlite)?;

    #[derive(Default)]
    struct RepoCounts {
        repository_name: String,
        total: u64,
        succeeded: u64,
        failed: u64,
        running: u64,
    }

    let mut repo_map: BTreeMap<(ConnectionId, ResourceId), RepoCounts> = BTreeMap::new();

    for row in rows {
        let (conn_id_str, repo_id_str, repo_name, _run_id, attrs_json) =
            row.map_err(StoreError::Sqlite)?;
        let conn_id: ConnectionId = wrapped(conn_id_str).map_err(StoreError::Sqlite)?;
        let repo_id: ResourceId = wrapped(repo_id_str).map_err(StoreError::Sqlite)?;

        let entry = repo_map
            .entry((conn_id.clone(), repo_id))
            .or_insert_with(|| RepoCounts {
                repository_name: repo_name,
                ..Default::default()
            });
        entry.total += 1;

        let attrs: serde_json::Value = serde_json::from_str(&attrs_json)
            .map_err(|_| StoreError::Contract("invalid workflow_run attributes_json".into()))?;

        let status = attrs
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("completed");
        let conclusion = attrs.get("conclusion").and_then(|v| v.as_str());

        if status != "completed" {
            entry.running += 1;
        } else if conclusion == Some("success") {
            entry.succeeded += 1;
        } else if conclusion == Some("failure") {
            entry.failed += 1;
        }
    }

    let mut result: Vec<GitHubActionsSummaryRow> = Vec::new();
    for ((conn_id, repo_id), counts) in repo_map {
        let conn_name = github_connections
            .get(&conn_id)
            .cloned()
            .unwrap_or_default();
        result.push(GitHubActionsSummaryRow {
            connection_id: conn_id,
            connection_name: conn_name,
            repository_id: repo_id,
            repository_name: counts.repository_name,
            action_count: counts.total,
            succeeded: counts.succeeded,
            failed: counts.failed,
            running: counts.running,
        });
    }

    result.sort_by(|a, b| {
        a.connection_name
            .cmp(&b.connection_name)
            .then(a.repository_name.cmp(&b.repository_name))
    });

    Ok(result)
}

fn grouped_counts(
    connection: &SqliteConnection,
    sql: &str,
) -> Result<Vec<(String, u64)>, StoreError> {
    let mut statement = connection.prepare(sql).map_err(StoreError::Sqlite)?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(StoreError::Sqlite)?;
    rows.map(|row| {
        let (value, count) = row.map_err(StoreError::Sqlite)?;
        Ok((
            value,
            u64::try_from(count)
                .map_err(|_| StoreError::Contract("negative aggregate count".into()))?,
        ))
    })
    .collect()
}

fn push_text_set(sql: &mut String, values: &mut Vec<Value>, column: &str, set: &BTreeSet<String>) {
    if set.is_empty() {
        return;
    }
    sql.push_str(&format!(" AND {column} IN ({})", placeholders(set.len())));
    values.extend(set.iter().cloned().map(Value::Text));
}

fn push_freshness_filter(
    sql: &mut String,
    values: &mut Vec<Value>,
    requested: &[Freshness],
    cutoffs: &BTreeMap<ConnectionId, FreshnessCutoffs>,
) {
    let requested_values = [Freshness::Fresh, Freshness::Stale, Freshness::Expired]
        .into_iter()
        .filter(|value| requested.contains(value))
        .collect::<Vec<_>>();
    if requested_values.is_empty() || requested_values.len() == 3 {
        return;
    }
    sql.push_str(" AND (");
    for (index, (connection_id, cutoff)) in cutoffs.iter().enumerate() {
        if index > 0 {
            sql.push_str(" OR ");
        }
        sql.push_str("(r.connection_id = ? AND (");
        values.push(Value::Text(connection_id.as_str().to_owned()));
        for (branch, freshness) in requested_values.iter().enumerate() {
            if branch > 0 {
                sql.push_str(" OR ");
            }
            match freshness {
                Freshness::Fresh => {
                    sql.push_str("r.last_seen_at >= ?");
                    values.push(Value::Integer(cutoff.fresh_after_millis));
                }
                Freshness::Stale => {
                    sql.push_str("(r.last_seen_at < ? AND r.last_seen_at >= ?)");
                    values.push(Value::Integer(cutoff.fresh_after_millis));
                    values.push(Value::Integer(cutoff.expired_after_millis));
                }
                Freshness::Expired => {
                    sql.push_str("r.last_seen_at < ?");
                    values.push(Value::Integer(cutoff.expired_after_millis));
                }
            }
        }
        sql.push_str("))");
    }
    sql.push(')');
}

fn classify_freshness(
    resource: &Resource,
    cutoffs: &BTreeMap<ConnectionId, FreshnessCutoffs>,
) -> Result<Freshness, StoreError> {
    let cutoff = cutoffs
        .get(&resource.connection_id)
        .ok_or_else(|| StoreError::Contract("freshness cutoff disappeared during query".into()))?;
    Ok(classify_millis(resource.last_seen_at.unix_millis(), cutoff))
}

fn classify_millis(last_seen_at: i64, cutoff: &FreshnessCutoffs) -> Freshness {
    if last_seen_at >= cutoff.fresh_after_millis {
        Freshness::Fresh
    } else if last_seen_at >= cutoff.expired_after_millis {
        Freshness::Stale
    } else {
        Freshness::Expired
    }
}

fn freshness_index(value: Freshness) -> usize {
    match value {
        Freshness::Fresh => 0,
        Freshness::Stale => 1,
        Freshness::Expired => 2,
    }
}

fn placeholders(count: usize) -> String {
    std::iter::repeat_n("?", count)
        .collect::<Vec<_>>()
        .join(",")
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn change_cursor(change: &Change) -> String {
    format!(
        "{}:{}",
        change.observed_at.unix_millis(),
        change.change_id.as_str()
    )
}

fn parse_change_cursor(value: &str) -> Result<(i64, String), StoreError> {
    let (observed_at, change_id) = value
        .split_once(':')
        .ok_or_else(|| StoreError::Contract("change cursor is invalid".into()))?;
    let observed_at = observed_at
        .parse::<i64>()
        .map_err(|_| StoreError::Contract("change cursor is invalid".into()))?;
    if change_id.is_empty() {
        return Err(StoreError::Contract("change cursor is invalid".into()));
    }
    Ok((observed_at, change_id.to_owned()))
}

fn resource_health_text(value: ResourceHealth) -> &'static str {
    match value {
        ResourceHealth::Healthy => "healthy",
        ResourceHealth::Degraded => "degraded",
        ResourceHealth::Unhealthy => "unhealthy",
        ResourceHealth::Unknown => "unknown",
    }
}

fn resource_health_value(value: &str) -> Result<ResourceHealth, StoreError> {
    match value {
        "healthy" => Ok(ResourceHealth::Healthy),
        "degraded" => Ok(ResourceHealth::Degraded),
        "unhealthy" => Ok(ResourceHealth::Unhealthy),
        "unknown" => Ok(ResourceHealth::Unknown),
        _ => Err(StoreError::Contract("unknown resource health".into())),
    }
}

fn connector_health_value(value: &str) -> Result<ConnectorHealth, StoreError> {
    match value {
        "healthy" => Ok(ConnectorHealth::Healthy),
        "degraded" => Ok(ConnectorHealth::Degraded),
        "auth_failed" => Ok(ConnectorHealth::AuthFailed),
        "rate_limited" => Ok(ConnectorHealth::RateLimited),
        "unreachable" => Ok(ConnectorHealth::Unreachable),
        "disabled" => Ok(ConnectorHealth::Disabled),
        _ => Err(StoreError::Contract("unknown connector health".into())),
    }
}
