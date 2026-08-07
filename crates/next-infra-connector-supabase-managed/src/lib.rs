//! Supabase Management API read contract. This crate intentionally has no
//! self-hosted or Data API assumptions.

use async_trait::async_trait;
use next_infra_connector_api::{ConnectorDescriptor, ResourceObservation};
use next_infra_core::{
    ConnectorCoverage, ConnectorCoverageLevel, ConnectorType, ExternalId, LabelKey, ResourceHealth,
    ResourceKind, SchemaVersion, Scope, SyncMode, Timestamp,
};
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;

pub const MANAGEMENT_API_ORIGIN: &str = "https://api.supabase.com";

#[async_trait]
pub trait ManagementTransport: Send + Sync {
    async fn get(
        &self,
        request: ManagementRequest,
    ) -> Result<Vec<u8>, next_infra_connector_api::ConnectorFailure>;
}

pub struct SupabaseManagedConnector<T> {
    descriptor: next_infra_connector_api::ConnectorDescriptor,
    transport: T,
}

impl<T> SupabaseManagedConnector<T> {
    pub fn new(transport: T) -> Self {
        Self {
            descriptor: descriptor(),
            transport,
        }
    }
}

#[async_trait]
impl<T: ManagementTransport> next_infra_connector_api::ReadConnector
    for SupabaseManagedConnector<T>
{
    fn descriptor(&self) -> &next_infra_connector_api::ConnectorDescriptor {
        &self.descriptor
    }
    async fn validate(
        &self,
        request: next_infra_connector_api::ValidationRequest,
        secret: Option<&next_infra_core::SecretValue>,
    ) -> next_infra_connector_api::ConnectorResult<next_infra_connector_api::ValidationReport> {
        let mut errors = validate_connection(&request.connection, &self.descriptor);
        if secret.is_none() {
            errors.push(next_infra_connector_api::ValidationIssue {
                code: next_infra_core::ErrorCode::CredentialUnavailable,
                message: "Supabase Management API credential is unavailable".into(),
            });
        }
        if !errors.is_empty() {
            return Ok(next_infra_connector_api::ValidationReport {
                status: next_infra_connector_api::ValidationStatus::Invalid,
                warnings: vec![],
                errors,
            });
        }
        Ok(next_infra_connector_api::ValidationReport {
            status: next_infra_connector_api::ValidationStatus::Valid,
            warnings: vec![],
            errors: vec![],
        })
    }
    async fn sync(
        &self,
        request: next_infra_connector_api::SyncRequest,
        secret: Option<&next_infra_core::SecretValue>,
    ) -> next_infra_connector_api::ConnectorResult<next_infra_connector_api::SyncOutcome> {
        if let Some(issue) = validate_connection(&request.connection, &self.descriptor)
            .into_iter()
            .next()
        {
            return Err(next_infra_connector_api::ConnectorFailure {
                code: issue.code,
                message: issue.message,
                retryable: false,
                retry_after_ms: None,
            });
        }
        let secret = secret.ok_or(next_infra_connector_api::ConnectorFailure {
            code: next_infra_core::ErrorCode::CredentialUnavailable,
            message: "Supabase Management API credential is unavailable".into(),
            retryable: false,
            retry_after_ms: None,
        })?;
        let body = self
            .transport
            .get(ManagementRequest::new("/v1/projects", secret).map_err(|_| invalid_response())?)
            .await?;
        #[derive(Deserialize)]
        struct Envelope {
            organizations: Vec<OrganizationDto>,
            projects: Vec<ProjectDto>,
        }
        let envelope: Envelope = serde_json::from_slice(&body).map_err(|_| invalid_response())?;
        let mut resources = map_organizations(
            &request.scope,
            Timestamp::from_unix_millis(0).unwrap(),
            envelope.organizations,
        )
        .map_err(|_| invalid_response())?;
        resources.extend(
            map_projects(
                &request.scope,
                Timestamp::from_unix_millis(0).unwrap(),
                envelope.projects,
            )
            .map_err(|_| invalid_response())?,
        );
        resources.sort_by_key(|r| (r.kind.clone(), r.external_id.clone()));
        let relations = managed_relations(&resources);
        let batch = next_infra_connector_api::ObservationBatch {
            resources,
            relations,
            coverage: next_infra_core::SyncCoverage::AuthoritativeFull {
                scope: request.scope.clone(),
            },
            next_cursor: None,
            warnings: vec![],
            redaction_report: Default::default(),
            provider_request_summary: next_infra_connector_api::ProviderRequestSummary {
                request_count: 1,
                ..Default::default()
            },
        };
        let outcome = next_infra_connector_api::SyncOutcome::Complete { batch };
        outcome
            .validate_for(&request)
            .map_err(|_| invalid_response())?;
        Ok(outcome)
    }
}

fn validate_connection(
    connection: &next_infra_connector_api::ConnectionInput,
    descriptor: &next_infra_connector_api::ConnectorDescriptor,
) -> Vec<next_infra_connector_api::ValidationIssue> {
    let mut errors = vec![];
    if connection.connector_type != descriptor.connector_type {
        errors.push(next_infra_connector_api::ValidationIssue {
            code: next_infra_core::ErrorCode::InvalidDomainValue,
            message: "Supabase managed connector type mismatch".into(),
        });
    }
    if connection.config_schema_version != descriptor.config_schema_version
        || !connection
            .config
            .as_object()
            .is_some_and(serde_json::Map::is_empty)
    {
        errors.push(next_infra_connector_api::ValidationIssue {
            code: next_infra_core::ErrorCode::SchemaIncompatible,
            message: "Supabase managed config schema is unsupported".into(),
        });
    }
    errors
}
fn invalid_response() -> next_infra_connector_api::ConnectorFailure {
    next_infra_connector_api::ConnectorFailure {
        code: next_infra_core::ErrorCode::InvalidResponse,
        message: "Supabase Management API response is invalid".into(),
        retryable: false,
        retry_after_ms: None,
    }
}

fn managed_relations(
    resources: &[ResourceObservation],
) -> Vec<next_infra_connector_api::RelationObservation> {
    let mut relations = Vec::new();
    for project in resources
        .iter()
        .filter(|resource| resource.kind.as_str() == "supabase.managed.project")
    {
        let Some(organization_id) = project
            .attributes
            .get("organization_id")
            .and_then(|value| value.as_str())
        else {
            continue;
        };
        let source_id = ExternalId::new(format!("supabase.managed.organization:{organization_id}"))
            .expect("validated provider ID");
        if !resources.iter().any(|resource| {
            resource.kind.as_str() == "supabase.managed.organization"
                && resource.external_id == source_id
        }) {
            continue;
        }
        relations.push(next_infra_connector_api::RelationObservation {
            source: next_infra_connector_api::ResourceLocator {
                kind: ResourceKind::new("supabase.managed.organization").unwrap(),
                external_id: source_id,
            },
            target: next_infra_connector_api::ResourceLocator {
                kind: project.kind.clone(),
                external_id: project.external_id.clone(),
            },
            kind: next_infra_core::RelationKind::new("supabase.contains").unwrap(),
            evidence_key: next_infra_core::EvidenceKey::new(format!(
                "supabase:contains:{}",
                project.external_id
            ))
            .unwrap(),
            field_path: next_infra_core::FieldPath::new("organization_id").unwrap(),
            observed_at: project.observed_at,
        });
    }
    relations
}

pub struct ManagementRequest {
    pub url: url::Url,
    pub authorization: reqwest::header::HeaderValue,
}

impl ManagementRequest {
    pub fn new(path: &str, secret: &next_infra_core::SecretValue) -> Result<Self, String> {
        if !path.starts_with('/') || path.contains(['?', '#']) {
            return Err("invalid Management API path".into());
        }
        let url = url::Url::parse(MANAGEMENT_API_ORIGIN)
            .unwrap()
            .join(path)
            .map_err(|_| "invalid Management API path")?;
        let token =
            std::str::from_utf8(secret.expose()).map_err(|_| "invalid Management API token")?;
        let mut authorization = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| "invalid Management API token")?;
        authorization.set_sensitive(true);
        Ok(Self { url, authorization })
    }
}

impl std::fmt::Debug for ManagementRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ManagementRequest")
            .field("path", &self.url.path())
            .field("authorization", &"[REDACTED]")
            .finish()
    }
}

pub fn descriptor() -> ConnectorDescriptor {
    let organization = ResourceKind::new("supabase.managed.organization").unwrap();
    let project = ResourceKind::new("supabase.managed.project").unwrap();
    ConnectorDescriptor {
        connector_type: ConnectorType::new("supabase-managed").unwrap(),
        connector_version: "1.0.0".into(),
        config_schema_version: SchemaVersion::new(1).unwrap(),
        auth: next_infra_connector_api::AuthDescriptor {
            kind: next_infra_connector_api::AuthKind::Token,
            minimum_permissions: vec!["supabase.management.projects.read".into()],
        },
        sync_modes: vec![SyncMode::Full, SyncMode::Targeted],
        resources: vec![
            cap(
                organization.clone(),
                "supabase.managed.organizations",
                ConnectorCoverageLevel::Supported,
            ),
            cap(
                project.clone(),
                "supabase.managed.projects",
                ConnectorCoverageLevel::Supported,
            ),
        ],
        relations: vec![next_infra_connector_api::RelationCapability {
            source_kind: organization.clone(),
            target_kind: project.clone(),
            kind: next_infra_core::RelationKind::new("supabase.contains").unwrap(),
            coverage: ConnectorCoverage {
                module: "supabase.managed.organization_project".into(),
                level: ConnectorCoverageLevel::Supported,
                reason: None,
            },
        }],
        sensitive_field_policy: vec![
            "Management API token is transient SecretValue".into(),
            "database credentials, service role keys, logs and connection strings are unsupported"
                .into(),
        ],
        rate_limit: next_infra_connector_api::RateLimitGuidance {
            default_max_concurrency: 2,
            requests_per_minute: None,
            respects_retry_after: true,
        },
        recommended_sync_interval_secs: 900,
        known_gaps: vec![
            "Billing, logs, SQL, write APIs and self-hosted control plane are unsupported".into(),
        ],
    }
}

fn cap(
    kind: ResourceKind,
    module: &str,
    level: ConnectorCoverageLevel,
) -> next_infra_connector_api::ResourceCapability {
    next_infra_connector_api::ResourceCapability {
        kind,
        attribute_schema_version: SchemaVersion::new(1).unwrap(),
        coverage: ConnectorCoverage {
            module: module.into(),
            level,
            reason: None,
        },
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct OrganizationDto {
    pub id: String,
    pub name: String,
}
#[derive(Clone, Debug, Deserialize)]
pub struct ProjectDto {
    pub id: String,
    pub name: String,
    pub organization_id: Option<String>,
    pub region: Option<String>,
    pub status: Option<String>,
}

pub fn map_organizations(
    scope: &Scope,
    at: Timestamp,
    values: impl IntoIterator<Item = OrganizationDto>,
) -> Result<Vec<ResourceObservation>, String> {
    values
        .into_iter()
        .map(|v| {
            map(
                "supabase.managed.organization",
                &v.id,
                &v.name,
                scope,
                at,
                json!({}),
            )
        })
        .collect()
}
pub fn map_projects(
    scope: &Scope,
    at: Timestamp,
    values: impl IntoIterator<Item = ProjectDto>,
) -> Result<Vec<ResourceObservation>, String> {
    values.into_iter().map(|v| map("supabase.managed.project", &v.id, &v.name, scope, at, json!({"organization_id": v.organization_id, "region": v.region, "status": v.status}))).collect()
}
fn map(
    kind: &str,
    id: &str,
    name: &str,
    scope: &Scope,
    at: Timestamp,
    attributes: serde_json::Value,
) -> Result<ResourceObservation, String> {
    if id.is_empty() || name.is_empty() {
        return Err("Supabase managed identity is invalid".into());
    }
    Ok(ResourceObservation {
        kind: ResourceKind::new(kind).map_err(|_| "invalid kind")?,
        external_id: ExternalId::new(format!("{kind}:{id}")).map_err(|_| "invalid id")?,
        name: id.into(),
        display_name: name.into(),
        scope: scope.clone(),
        labels: BTreeMap::from([(
            LabelKey::new("supabase.control_plane").unwrap(),
            "managed".into(),
        )]),
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
    use std::sync::Mutex;
    #[test]
    fn management_request_is_fixed_and_redacted() {
        let request = ManagementRequest::new(
            "/v1/projects",
            &next_infra_core::SecretValue::new("fixture-token"),
        )
        .unwrap();
        assert_eq!(
            request.url.origin().ascii_serialization(),
            MANAGEMENT_API_ORIGIN
        );
        assert!(request.authorization.is_sensitive());
        assert!(!format!("{request:?}").contains("fixture-token"));
        assert!(
            ManagementRequest::new(
                "/v1/projects?token=secret",
                &next_infra_core::SecretValue::new("fixture-token"),
            )
            .is_err()
        );
    }
    struct FakeManagementTransport {
        body: Mutex<Vec<u8>>,
    }
    #[async_trait]
    impl ManagementTransport for FakeManagementTransport {
        async fn get(
            &self,
            request: ManagementRequest,
        ) -> Result<Vec<u8>, next_infra_connector_api::ConnectorFailure> {
            assert_eq!(request.url.path(), "/v1/projects");
            assert!(request.authorization.is_sensitive());
            Ok(self.body.lock().unwrap().clone())
        }
    }
    fn sync_request() -> next_infra_connector_api::SyncRequest {
        next_infra_connector_api::SyncRequest {
            sync_run_id: next_infra_core::SyncRunId::new("supabase-managed-fixture-run").unwrap(),
            connection: next_infra_connector_api::ConnectionInput {
                connection_id: next_infra_core::ConnectionId::new(
                    "supabase-managed-fixture-connection",
                )
                .unwrap(),
                connector_type: next_infra_core::ConnectorType::new("supabase-managed").unwrap(),
                config: serde_json::json!({}),
                config_schema_version: next_infra_core::SchemaVersion::new(1).unwrap(),
            },
            mode: next_infra_core::SyncMode::Full,
            scope: next_infra_core::Scope::new("supabase-managed-fixture-scope").unwrap(),
            cursor: None,
            targeted_resources: vec![],
        }
    }
    #[tokio::test]
    async fn read_connector_replays_allowlisted_management_summary() {
        let connector = SupabaseManagedConnector::new(FakeManagementTransport { body: Mutex::new(br#"{"organizations":[{"id":"org-1","name":"Fixture Org","token":"must-not-appear"}],"projects":[{"id":"project-1","name":"Fixture Project","organization_id":"org-1","region":"ap-example-1","secret":"must-not-appear"}]}"#.to_vec()) });
        let outcome = connector
            .sync(
                sync_request(),
                Some(&next_infra_core::SecretValue::new("fixture-token")),
            )
            .await
            .unwrap();
        let next_infra_connector_api::SyncOutcome::Complete { batch } = outcome else {
            panic!("expected complete")
        };
        assert_eq!(batch.resources.len(), 2);
        assert_eq!(batch.relations.len(), 1);
        let serialized = serde_json::to_string(&batch).unwrap();
        assert!(!serialized.contains("must-not-appear"), "{serialized}");
    }
    #[test]
    fn managed_descriptor_is_separate_and_read_only() {
        let d = descriptor();
        assert!(d.validate().is_ok());
        assert!(check_descriptor(&d).is_empty());
        assert_eq!(d.connector_type.as_str(), "supabase-managed");
        assert!(serde_json::to_string(&d).unwrap().contains("service role"));
    }
    #[test]
    fn mapper_drops_unknown_fields_and_uses_managed_identity() {
        let p: ProjectDto = serde_json::from_str(r#"{"id":"project-1","name":"Fixture project","region":"ap-southeast-1","secret":"drop"}"#).unwrap();
        let out = map_projects(
            &Scope::new("fixture-scope").unwrap(),
            Timestamp::from_unix_millis(1).unwrap(),
            [p],
        )
        .unwrap();
        let s = serde_json::to_string(&out).unwrap();
        assert!(s.contains("supabase.managed.project:project-1"));
        assert!(!s.contains("drop"));
    }
}
