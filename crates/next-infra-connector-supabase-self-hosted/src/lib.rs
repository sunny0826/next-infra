//! Self-hosted Supabase sources. No managed API DTOs or arbitrary SSH commands.
//!
//! Data surface: PostgREST OpenAPI document at GET /rest/v1/ — standard self-hosted
//! Supabase exposes PostgREST at /rest/v1/. The OpenAPI spec lists every exposed
//! table/schema. We collect one local `supabase.self_hosted.instance` resource
//! and one `supabase.self_hosted.table` resource per exposed table.

use async_trait::async_trait;
use next_infra_connector_api::{
    RelationObservation, ResourceLocator, ResourceObservation, SyncOutcome,
};
use next_infra_core::{
    ConnectorCoverage, ConnectorCoverageLevel, ConnectorType, CoverageGapReason, EvidenceKey,
    ExternalId, FieldPath, LabelKey, RelationKind, ResourceHealth, ResourceKind, SchemaVersion,
    Scope, SyncMode, Timestamp,
};
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;

const MAX_TABLES: usize = 500;
const DEFAULT_INSTANCE_DISPLAY_NAME: &str = "Supabase Self-hosted";

/// Auth role extracted from OpenAPI security scheme (if present).
#[derive(Clone, Debug, Default, Deserialize)]
#[allow(dead_code)]
struct OpenApiSpec {
    #[serde(default)]
    paths: BTreeMap<String, serde_json::Value>,
    #[serde(default, skip_serializing)]
    definitions: BTreeMap<String, serde_json::Value>,
    #[serde(default, skip_serializing)]
    components: Components,
    #[serde(default)]
    security: Vec<serde_json::Value>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[allow(dead_code)]
struct Components {
    #[serde(default, skip_serializing)]
    schemas: BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    OpenApi,
}

impl SourceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OpenApi => "openapi",
        }
    }
}

#[async_trait]
pub trait SelfHostedTransport: Send + Sync {
    async fn read_openapi(&self) -> Result<Vec<u8>, next_infra_connector_api::ConnectorFailure>;
}

pub struct SupabaseSelfHostedConnector<T> {
    descriptor: next_infra_connector_api::ConnectorDescriptor,
    transport: T,
}

impl<T> SupabaseSelfHostedConnector<T> {
    pub fn new(transport: T) -> Self {
        Self {
            descriptor: descriptor(),
            transport,
        }
    }
}

#[async_trait]
impl<T: SelfHostedTransport> next_infra_connector_api::ReadConnector
    for SupabaseSelfHostedConnector<T>
{
    fn descriptor(&self) -> &next_infra_connector_api::ConnectorDescriptor {
        &self.descriptor
    }

    async fn validate(
        &self,
        request: next_infra_connector_api::ValidationRequest,
        _secret: Option<&next_infra_core::SecretValue>,
    ) -> next_infra_connector_api::ConnectorResult<next_infra_connector_api::ValidationReport> {
        let mut errors = vec![];
        if request.connection.connector_type != self.descriptor.connector_type {
            errors.push(issue(
                next_infra_core::ErrorCode::InvalidDomainValue,
                "Supabase self-hosted connector type mismatch",
            ));
        }
        Ok(next_infra_connector_api::ValidationReport {
            status: if errors.is_empty() {
                next_infra_connector_api::ValidationStatus::Valid
            } else {
                next_infra_connector_api::ValidationStatus::Invalid
            },
            warnings: vec![],
            errors,
        })
    }

    async fn sync(
        &self,
        request: next_infra_connector_api::SyncRequest,
        _secret: Option<&next_infra_core::SecretValue>,
    ) -> next_infra_connector_api::ConnectorResult<next_infra_connector_api::SyncOutcome> {
        if request.connection.connector_type != self.descriptor.connector_type {
            return Err(next_infra_connector_api::ConnectorFailure {
                code: next_infra_core::ErrorCode::InvalidDomainValue,
                message: "Supabase self-hosted connector type mismatch".into(),
                retryable: false,
                retry_after_ms: None,
            });
        }

        let at = Timestamp::from_unix_millis(0).unwrap();
        let body = self.transport.read_openapi().await?;
        let spec: OpenApiSpec = serde_json::from_slice(&body).map_err(|_| invalid_response())?;

        // Extract auth role from OpenAPI security scheme, if present.
        let auth_role = extract_auth_role(&spec);

        let tables = parse_tables_from_openapi(&spec);
        let truncated = tables.len() > MAX_TABLES;
        let table_resources: Vec<ResourceObservation> = tables
            .into_iter()
            .take(MAX_TABLES)
            .map(|(_, (table_name, schema))| {
                map_table(&request.scope, at, table_name, schema, auth_role.as_deref())
            })
            .collect::<Result<_, _>>()
            .map_err(|_| invalid_response())?;
        let display_name = configured_display_name(&request.connection.config)
            .unwrap_or(DEFAULT_INSTANCE_DISPLAY_NAME);
        let instance =
            map_instance(&request.scope, display_name, at).map_err(|_| invalid_response())?;
        let relations = map_table_relations(&instance, &table_resources);
        let mut resources = Vec::with_capacity(table_resources.len() + 1);
        resources.push(instance);
        resources.extend(table_resources);

        let (coverage, outcome) = if truncated {
            let reason = CoverageGapReason::PaginationIncomplete;
            let count = resources.len() - 1;
            (
                next_infra_core::SyncCoverage::Partial {
                    scope: Some(request.scope.clone()),
                    reason: reason.clone(),
                },
                SyncOutcome::Partial {
                    batch: next_infra_connector_api::ObservationBatch {
                        resources,
                        relations,
                        coverage: next_infra_core::SyncCoverage::Partial {
                            scope: Some(request.scope.clone()),
                            reason,
                        },
                        next_cursor: None,
                        warnings: vec![next_infra_connector_api::ObservationWarning {
                            code: next_infra_core::ErrorCode::PartialPagination,
                            message: format!(
                                "Table inventory truncated: {} tables found, limit {}",
                                count, MAX_TABLES
                            ),
                        }],
                        redaction_report: Default::default(),
                        provider_request_summary: Default::default(),
                    },
                    failure: next_infra_connector_api::ConnectorFailure {
                        code: next_infra_core::ErrorCode::PartialPagination,
                        message: "Table inventory truncated".into(),
                        retryable: true,
                        retry_after_ms: None,
                    },
                },
            )
        } else {
            (
                next_infra_core::SyncCoverage::AuthoritativeFull {
                    scope: request.scope.clone(),
                },
                SyncOutcome::Complete {
                    batch: next_infra_connector_api::ObservationBatch {
                        resources,
                        relations,
                        coverage: next_infra_core::SyncCoverage::AuthoritativeFull {
                            scope: request.scope.clone(),
                        },
                        next_cursor: None,
                        warnings: vec![],
                        redaction_report: Default::default(),
                        provider_request_summary: Default::default(),
                    },
                },
            )
        };

        let _ = coverage;
        outcome
            .validate_for(&request)
            .map_err(|_| invalid_response())?;
        Ok(outcome)
    }
}

/// Extract the auth role name from the OpenAPI security scheme.
/// PostgREST sets the auth role via the OpenAPI securityDefinitions or the
/// `security` top-level field with a JWT/apikey scheme.
fn extract_auth_role(spec: &OpenApiSpec) -> Option<String> {
    let security = spec.security.first()?;
    let obj = security.as_object()?;
    let key = obj.keys().next()?;
    Some(key.clone())
}

/// Parse table-like paths from the OpenAPI `paths` object.
///
/// Heuristic: each top-level path key is treated as a table route.
/// Paths starting with "_" (PostgREST admin routes like "/_health") or "/"
/// alone are skipped. All other paths become table resources.
/// Example: "/users" → table "users"; "/public.profiles" → table "profiles" (schema "public").
fn parse_tables_from_openapi(spec: &OpenApiSpec) -> Vec<(String, (String, Option<String>))> {
    spec.paths
        .iter()
        .filter(|(path, _)| {
            // Skip root and admin routes
            !path.is_empty()
                && path.as_str() != "/"
                && !path.starts_with("/_")
                && path.starts_with('/')
        })
        .map(|(path, _)| {
            // Extract table name: last segment after '/', drop any schema prefix.
            // "/public.users" → ("users", Some("public"))
            // "/users" → ("users", None)
            let segments: Vec<&str> = path.trim_start_matches('/').split('.').collect();
            let table_name = if segments.len() == 2 {
                segments[1].to_string()
            } else {
                segments
                    .last()
                    .map(|s| (*s).to_string())
                    .unwrap_or_else(|| path.clone())
            };
            let schema_name = if segments.len() == 2 {
                Some(segments[0].to_string())
            } else {
                None
            };
            let external_id = sanitize_external_id(path);
            (external_id, (table_name, schema_name))
        })
        .collect()
}

/// Build a stable external ID from a path string.
/// Replaces non-alphanumeric chars with underscores and prefixes to avoid
/// leading-digit issues.
fn sanitize_external_id(path: &str) -> String {
    let sanitized: String = path
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '.')
        .collect();
    format!(
        "table_{}",
        sanitized.trim_start_matches('/').replace('.', "_")
    )
}

fn issue(
    code: next_infra_core::ErrorCode,
    message: &str,
) -> next_infra_connector_api::ValidationIssue {
    next_infra_connector_api::ValidationIssue {
        code,
        message: message.into(),
    }
}

fn invalid_response() -> next_infra_connector_api::ConnectorFailure {
    next_infra_connector_api::ConnectorFailure {
        code: next_infra_core::ErrorCode::InvalidResponse,
        message: "Supabase self-hosted source response is invalid".into(),
        retryable: false,
        retry_after_ms: None,
    }
}

pub fn descriptor() -> next_infra_connector_api::ConnectorDescriptor {
    let instance_kind = ResourceKind::new("supabase.self_hosted.instance").unwrap();
    let table_kind = ResourceKind::new("supabase.self_hosted.table").unwrap();
    next_infra_connector_api::ConnectorDescriptor {
        connector_type: ConnectorType::new("supabase-self-hosted").unwrap(),
        connector_version: "1.0.0".into(),
        config_schema_version: SchemaVersion::new(1).unwrap(),
        auth: next_infra_connector_api::AuthDescriptor {
            kind: next_infra_connector_api::AuthKind::Token,
            minimum_permissions: vec!["PostgREST /rest/v1/ read (apikey)".into()],
        },
        sync_modes: vec![SyncMode::Full, SyncMode::Targeted],
        resources: vec![
            cap(
                instance_kind.clone(),
                "supabase.self_hosted.instance",
                ConnectorCoverageLevel::Supported,
                None,
            ),
            cap(
                table_kind.clone(),
                "supabase.self_hosted.tables",
                ConnectorCoverageLevel::Partial,
                Some("PostgREST-exposed table inventory; user-defined schema varies by instance"),
            ),
        ],
        relations: vec![next_infra_connector_api::RelationCapability {
            kind: RelationKind::new("supabase.contains").unwrap(),
            source_kind: instance_kind,
            target_kind: table_kind,
            coverage: ConnectorCoverage {
                module: "supabase.self_hosted.instance_tables".into(),
                level: ConnectorCoverageLevel::Partial,
                reason: Some(
                    "Relations cover the bounded PostgREST table inventory discovered in this sync"
                        .into(),
                ),
            },
        }],
        sensitive_field_policy: vec![
            "No credentials, connection strings, or data rows are emitted. Only table names from the OpenAPI spec are included.".into()
        ],
        rate_limit: next_infra_connector_api::RateLimitGuidance {
            default_max_concurrency: 1,
            requests_per_minute: None,
            respects_retry_after: true,
        },
        recommended_sync_interval_secs: 900,
        known_gaps: vec![
            "No row data: only table names from PostgREST OpenAPI spec are collected.".into(),
            "No Supabase Studio API: the standard self-hosted stack does not expose a management API.".into(),
            "Instance gateway TLS-fingerprint filtering may cause reqwest 401 on some deployments (curl/OpenSSL works).".into(),
        ],
    }
}

fn cap(
    kind: ResourceKind,
    module: &str,
    level: ConnectorCoverageLevel,
    reason: Option<&str>,
) -> next_infra_connector_api::ResourceCapability {
    next_infra_connector_api::ResourceCapability {
        kind,
        attribute_schema_version: SchemaVersion::new(1).unwrap(),
        coverage: ConnectorCoverage {
            module: module.into(),
            level,
            reason: reason.map(str::to_owned),
        },
    }
}

/// Map the configured local scope to the single self-hosted instance resource.
///
/// The scope is the connector's local, non-sensitive display identity. No
/// provider URL, host, address, credential, or response content is copied.
pub fn map_instance(
    scope: &Scope,
    connection_display_name: &str,
    at: Timestamp,
) -> Result<ResourceObservation, String> {
    let connection_display_name = connection_display_name.trim();
    if connection_display_name.is_empty() || connection_display_name.len() > 120 {
        return Err("invalid display name".into());
    }
    let kind = ResourceKind::new("supabase.self_hosted.instance").map_err(|_| "invalid kind")?;
    let external_id = ExternalId::new(format!(
        "supabase.self_hosted.instance:{}",
        stable_scope_digest(scope)
    ))
    .map_err(|_| "invalid id")?;

    Ok(ResourceObservation {
        kind,
        external_id,
        name: connection_display_name.into(),
        display_name: connection_display_name.into(),
        scope: scope.clone(),
        labels: BTreeMap::from([
            (
                LabelKey::new("supabase.control_plane").unwrap(),
                "self_hosted".into(),
            ),
            (LabelKey::new("supabase.source").unwrap(), "openapi".into()),
        ]),
        health: ResourceHealth::Unknown,
        attributes: json!({}),
        attribute_schema_version: SchemaVersion::new(1).unwrap(),
        observed_at: at,
    })
}

fn configured_display_name(config: &serde_json::Value) -> Option<&str> {
    config
        .get("display_name")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.len() <= 120)
}

/// Return a stable, opaque identifier for the configured local scope.
///
/// `Scope` is intentionally permissive because it is a shared sync contract;
/// do not copy it into an external ID where a URL, host, or other sensitive
/// value could become part of the persisted resource identity.
fn stable_scope_digest(scope: &Scope) -> String {
    // FNV-1a is sufficient here: this is an opaque identity component, not a
    // security boundary, and keeping it local avoids adding a dependency.
    let mut digest = 0xcbf29ce484222325_u64;
    for byte in scope.as_str().as_bytes() {
        digest ^= u64::from(*byte);
        digest = digest.wrapping_mul(0x100000001b3_u64);
    }
    format!("{digest:016x}")
}

fn map_table_relations(
    instance: &ResourceObservation,
    tables: &[ResourceObservation],
) -> Vec<RelationObservation> {
    tables
        .iter()
        .map(|table| RelationObservation {
            source: ResourceLocator {
                kind: instance.kind.clone(),
                external_id: instance.external_id.clone(),
            },
            target: ResourceLocator {
                kind: table.kind.clone(),
                external_id: table.external_id.clone(),
            },
            kind: RelationKind::new("supabase.contains").expect("static relation kind"),
            evidence_key: EvidenceKey::new(format!("supabase:contains:{}", table.external_id))
                .expect("stable table evidence key"),
            field_path: FieldPath::new("paths").expect("static OpenAPI field path"),
            observed_at: table.observed_at,
        })
        .collect()
}

/// Map an OpenAPI path to a `supabase.self_hosted.table` resource.
pub fn map_table(
    scope: &Scope,
    at: Timestamp,
    table_name: String,
    schema: Option<String>,
    auth_role: Option<&str>,
) -> Result<ResourceObservation, String> {
    let mut attributes = json!({
        "table": table_name,
    });
    if let Some(s) = &schema {
        attributes["schema"] = json!(s);
    }
    if let Some(role) = auth_role {
        attributes["auth_role"] = json!(role);
    }

    let external_id = ExternalId::new(format!("supabase.self_hosted.table:{}", table_name))
        .map_err(|_| "invalid id")?;

    Ok(ResourceObservation {
        kind: ResourceKind::new("supabase.self_hosted.table").map_err(|_| "invalid kind")?,
        external_id,
        name: table_name.clone(),
        display_name: table_name,
        scope: scope.clone(),
        labels: BTreeMap::from([
            (
                LabelKey::new("supabase.control_plane").unwrap(),
                "self_hosted".into(),
            ),
            (LabelKey::new("supabase.source").unwrap(), "openapi".into()),
        ]),
        health: ResourceHealth::Unknown,
        attributes,
        attribute_schema_version: SchemaVersion::new(1).unwrap(),
        observed_at: at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use next_infra_connector_api::ReadConnector;
    use next_infra_connector_contract_tests::check_descriptor;
    use next_infra_core::SyncRunId;
    use std::sync::Mutex;

    #[test]
    fn descriptor_declares_instance_table_and_contains_relation() {
        let d = descriptor();
        assert!(d.validate().is_ok());
        assert!(check_descriptor(&d).is_empty());
        assert_eq!(d.resources.len(), 2);
        let instance = d
            .resources
            .iter()
            .find(|cap| cap.kind.as_str() == "supabase.self_hosted.instance")
            .expect("instance capability");
        assert_eq!(instance.coverage.level, ConnectorCoverageLevel::Supported);
        assert!(instance.coverage.reason.is_none());
        let table = d
            .resources
            .iter()
            .find(|cap| cap.kind.as_str() == "supabase.self_hosted.table")
            .expect("table capability");
        assert_eq!(table.coverage.module, "supabase.self_hosted.tables");

        assert_eq!(d.relations.len(), 1);
        let relation = &d.relations[0];
        assert_eq!(relation.kind.as_str(), "supabase.contains");
        assert_eq!(
            relation.source_kind.as_str(),
            "supabase.self_hosted.instance"
        );
        assert_eq!(relation.target_kind.as_str(), "supabase.self_hosted.table");
    }

    #[test]
    fn descriptor_known_gaps_mentions_tls_fingerprint() {
        let d = descriptor();
        assert!(
            d.known_gaps
                .iter()
                .any(|g| g.contains("TLS") || g.contains("reqwest"))
        );
    }

    #[test]
    fn sanitize_external_id_produces_valid_id() {
        assert_eq!(sanitize_external_id("/users"), "table_users");
        assert_eq!(
            sanitize_external_id("/public.profiles"),
            "table_public_profiles"
        );
        // Hyphens are filtered out
        assert_eq!(sanitize_external_id("/my-table"), "table_mytable");
    }

    #[test]
    fn instance_external_id_is_stable_and_opaque() {
        let scope = Scope::new("fixture-scope").unwrap();
        let at = Timestamp::from_unix_millis(0).unwrap();
        let first = map_instance(&scope, "Fixture Supabase", at).unwrap();
        let second = map_instance(&scope, "Fixture Supabase", at).unwrap();

        assert_eq!(first.external_id, second.external_id);
        assert!(
            first
                .external_id
                .as_str()
                .starts_with("supabase.self_hosted.instance:")
        );
        assert!(!first.external_id.as_str().contains(scope.as_str()));
        assert_eq!(first.name, "Fixture Supabase");
        assert_eq!(first.display_name, "Fixture Supabase");
        assert_ne!(first.display_name, scope.as_str());
    }

    #[test]
    fn parse_tables_from_openapi_skips_admin_routes() {
        let spec = OpenApiSpec {
            paths: BTreeMap::from([
                ("/users".into(), serde_json::json!({})),
                ("/public.profiles".into(), serde_json::json!({})),
                ("/_health".into(), serde_json::json!({})),
                ("/".into(), serde_json::json!({})),
            ]),
            ..Default::default()
        };
        let tables = parse_tables_from_openapi(&spec);
        let paths: Vec<&str> = tables.iter().map(|(p, _)| p.as_str()).collect();
        assert!(paths.contains(&"table_users"), "{paths:?}");
        assert!(paths.contains(&"table_public_profiles"), "{paths:?}");
        assert!(!paths.contains(&"table__health"), "{paths:?}");
        assert!(!paths.contains(&"table_"), "{paths:?}");
    }

    #[test]
    fn map_table_unknown_fields_dropped() {
        // Unknown fields in the OpenAPI path value should not cause issues;
        // parse_tables_from_openapi only reads path keys.
        let spec = OpenApiSpec {
            paths: BTreeMap::from([(
                "/users".into(),
                serde_json::json!({"x-custom-field": "should-be-ignored"}),
            )]),
            ..Default::default()
        };
        let tables = parse_tables_from_openapi(&spec);
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].0, "table_users");
        assert_eq!(tables[0].1, (String::from("users"), None));
    }

    struct FakeSelfHostedTransport {
        body: Mutex<Result<Vec<u8>, next_infra_connector_api::ConnectorFailure>>,
    }

    #[async_trait]
    impl SelfHostedTransport for FakeSelfHostedTransport {
        async fn read_openapi(
            &self,
        ) -> Result<Vec<u8>, next_infra_connector_api::ConnectorFailure> {
            self.body.lock().unwrap().clone()
        }
    }

    fn sync_request() -> next_infra_connector_api::SyncRequest {
        next_infra_connector_api::SyncRequest {
            sync_run_id: SyncRunId::new("fixture-run").unwrap(),
            connection: next_infra_connector_api::ConnectionInput {
                connection_id: next_infra_core::ConnectionId::new("fixture-connection").unwrap(),
                connector_type: next_infra_core::ConnectorType::new("supabase-self-hosted")
                    .unwrap(),
                config: serde_json::json!({ "base_url": "fixture-base" }),
                config_schema_version: SchemaVersion::new(1).unwrap(),
            },
            mode: SyncMode::Full,
            scope: Scope::new("fixture-scope").unwrap(),
            cursor: None,
            targeted_resources: vec![],
        }
    }

    fn assert_instance_batch(
        batch: &next_infra_connector_api::ObservationBatch,
        expected_table_count: usize,
    ) {
        assert_eq!(batch.resources.len(), expected_table_count + 1);
        assert_eq!(batch.relations.len(), expected_table_count);

        let instance = batch
            .resources
            .iter()
            .find(|resource| resource.kind.as_str() == "supabase.self_hosted.instance")
            .expect("instance resource");
        assert_eq!(instance.name, DEFAULT_INSTANCE_DISPLAY_NAME);
        assert_eq!(instance.display_name, DEFAULT_INSTANCE_DISPLAY_NAME);
        assert_ne!(instance.display_name, "fixture-scope");

        let tables = batch
            .resources
            .iter()
            .filter(|resource| resource.kind.as_str() == "supabase.self_hosted.table")
            .collect::<Vec<_>>();
        assert_eq!(tables.len(), expected_table_count);

        for relation in &batch.relations {
            assert_eq!(relation.kind.as_str(), "supabase.contains");
            assert_eq!(relation.source.kind, instance.kind);
            assert_eq!(relation.source.external_id, instance.external_id);
            assert_eq!(relation.target.kind.as_str(), "supabase.self_hosted.table");
            assert!(
                tables
                    .iter()
                    .any(|table| { table.external_id == relation.target.external_id })
            );
            assert!(
                relation
                    .evidence_key
                    .as_str()
                    .starts_with("supabase:contains:")
            );
            assert_eq!(relation.field_path.as_str(), "paths");
        }
    }

    #[tokio::test]
    async fn complete_outcome_when_under_limit() {
        let body = serde_json::json!({
            "paths": {
                "/users": {"description": "ignored-response-value"},
                "/posts": {}
            }
        })
        .to_string()
        .into_bytes();
        let connector = SupabaseSelfHostedConnector::new(FakeSelfHostedTransport {
            body: Mutex::new(Ok(body)),
        });
        let outcome = connector
            .sync(
                sync_request(),
                Some(&next_infra_core::SecretValue::new("fixture-token")),
            )
            .await
            .unwrap();

        match outcome {
            SyncOutcome::Complete { batch } => {
                assert_instance_batch(&batch, 2);
                assert!(matches!(
                    batch.coverage,
                    next_infra_core::SyncCoverage::AuthoritativeFull { .. }
                ));
                let json = serde_json::to_string(&batch).unwrap();
                assert!(!json.contains("fixture-token"));
                assert!(!json.contains("apikey"));
                assert!(!json.contains("ignored-response-value"));
            }
            other => panic!("expected Complete, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn partial_outcome_when_truncated() {
        // Build a spec with MAX_TABLES + 1 entries
        let mut paths = serde_json::Map::new();
        for i in 0..=MAX_TABLES {
            paths.insert(format!("/table_{}", i), serde_json::json!({}));
        }
        let body = serde_json::json!({ "paths": paths })
            .to_string()
            .into_bytes();
        let connector = SupabaseSelfHostedConnector::new(FakeSelfHostedTransport {
            body: Mutex::new(Ok(body)),
        });
        let outcome = connector
            .sync(
                sync_request(),
                Some(&next_infra_core::SecretValue::new("fixture-token")),
            )
            .await
            .unwrap();

        match outcome {
            SyncOutcome::Partial { batch, failure } => {
                assert_instance_batch(&batch, MAX_TABLES);
                assert!(matches!(
                    batch.coverage,
                    next_infra_core::SyncCoverage::Partial {
                        reason: CoverageGapReason::PaginationIncomplete,
                        ..
                    }
                ));
                assert!(!batch.warnings.is_empty());
                assert!(failure.message.contains("truncated"));
                let json = serde_json::to_string(&batch).unwrap();
                assert!(!json.contains("fixture-token"));
            }
            other => panic!("expected Partial, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn invalid_openapi_returns_error() {
        let connector = SupabaseSelfHostedConnector::new(FakeSelfHostedTransport {
            body: Mutex::new(Ok(b"not json".to_vec())),
        });
        let outcome = connector
            .sync(
                sync_request(),
                Some(&next_infra_core::SecretValue::new("fixture-token")),
            )
            .await;
        assert!(outcome.is_err());
    }

    #[tokio::test]
    async fn transport_error_returns_error() {
        let connector = SupabaseSelfHostedConnector::new(FakeSelfHostedTransport {
            body: Mutex::new(Err(invalid_response())),
        });
        let outcome = connector
            .sync(
                sync_request(),
                Some(&next_infra_core::SecretValue::new("fixture-token")),
            )
            .await;
        assert!(outcome.is_err());
    }

    #[tokio::test]
    async fn auth_role_extracted_from_security() {
        let spec = serde_json::from_str::<OpenApiSpec>(
            r#"{
              "paths": {"/users": {}},
              "security": [{ "apiKey": [] }]
            }"#,
        )
        .unwrap();
        let role = extract_auth_role(&spec);
        assert_eq!(role, Some("apiKey".to_string()));
    }

    #[tokio::test]
    async fn validate_keeps_v1_config_valid_and_rejects_wrong_connector_type() {
        let body = serde_json::json!({ "paths": { "/users": {} } })
            .to_string()
            .into_bytes();
        let connector = SupabaseSelfHostedConnector::new(FakeSelfHostedTransport {
            body: Mutex::new(Ok(body)),
        });

        let v1 = next_infra_connector_api::ValidationRequest {
            connection: sync_request().connection,
        };
        let report = connector.validate(v1, None).await.unwrap();
        assert!(matches!(
            report.status,
            next_infra_connector_api::ValidationStatus::Valid
        ));

        // Wrong connector type → Invalid
        let wrong_type = next_infra_connector_api::ValidationRequest {
            connection: next_infra_connector_api::ConnectionInput {
                connection_id: next_infra_core::ConnectionId::new("c").unwrap(),
                connector_type: next_infra_core::ConnectorType::new("other").unwrap(),
                config: serde_json::json!({}),
                config_schema_version: SchemaVersion::new(1).unwrap(),
            },
        };
        let report = connector.validate(wrong_type, None).await.unwrap();
        assert!(matches!(
            report.status,
            next_infra_connector_api::ValidationStatus::Invalid
        ));
    }
}
