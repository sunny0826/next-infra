//! Self-hosted Supabase sources. No managed API DTOs or arbitrary SSH commands.

use async_trait::async_trait;
use next_infra_connector_api::ResourceObservation;
use next_infra_core::{
    ConnectorCoverage, ConnectorCoverageLevel, ConnectorType, ExternalId, LabelKey, ResourceHealth,
    ResourceKind, SchemaVersion, Scope, SyncMode, Timestamp,
};
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    ServiceApi,
    PostgresMetadata,
    FixedSshProbe,
}
impl SourceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ServiceApi => "service_api",
            Self::PostgresMetadata => "postgres_metadata",
            Self::FixedSshProbe => "fixed_ssh_probe",
        }
    }
}

#[async_trait]
pub trait SelfHostedTransport: Send + Sync {
    async fn read(
        &self,
        source: SourceKind,
    ) -> Result<Vec<u8>, next_infra_connector_api::ConnectorFailure>;
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
        secret: Option<&next_infra_core::SecretValue>,
    ) -> next_infra_connector_api::ConnectorResult<next_infra_connector_api::ValidationReport> {
        let mut errors = vec![];
        if request.connection.connector_type != self.descriptor.connector_type {
            errors.push(issue(
                next_infra_core::ErrorCode::InvalidDomainValue,
                "Supabase self-hosted connector type mismatch",
            ));
        }
        if secret.is_none() {
            errors.push(issue(
                next_infra_core::ErrorCode::CredentialUnavailable,
                "Supabase self-hosted source credential is unavailable",
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
        secret: Option<&next_infra_core::SecretValue>,
    ) -> next_infra_connector_api::ConnectorResult<next_infra_connector_api::SyncOutcome> {
        if request.connection.connector_type != self.descriptor.connector_type {
            return Err(next_infra_connector_api::ConnectorFailure {
                code: next_infra_core::ErrorCode::InvalidDomainValue,
                message: "Supabase self-hosted connector type mismatch".into(),
                retryable: false,
                retry_after_ms: None,
            });
        }
        if secret.is_none() {
            return Err(next_infra_connector_api::ConnectorFailure {
                code: next_infra_core::ErrorCode::CredentialUnavailable,
                message: "Supabase self-hosted source credential is unavailable".into(),
                retryable: false,
                retry_after_ms: None,
            });
        }
        let at = Timestamp::from_unix_millis(0).unwrap();
        let mut resources = vec![];
        let mut failures = vec![];
        match self.transport.read(SourceKind::ServiceApi).await {
            Ok(body) => match serde_json::from_slice::<Vec<ServiceDto>>(&body) {
                Ok(values) => {
                    for value in values {
                        resources.push(
                            map_service(&request.scope, at, SourceKind::ServiceApi, value)
                                .map_err(|_| invalid_response())?,
                        );
                    }
                }
                Err(_) => failures.push(invalid_response()),
            },
            Err(error) => failures.push(error),
        }
        match self.transport.read(SourceKind::PostgresMetadata).await {
            Ok(body) => match serde_json::from_slice::<Vec<DatabaseDto>>(&body) {
                Ok(values) => {
                    for value in values {
                        resources.push(
                            map_database(&request.scope, at, SourceKind::PostgresMetadata, value)
                                .map_err(|_| invalid_response())?,
                        );
                    }
                }
                Err(_) => failures.push(invalid_response()),
            },
            Err(error) => failures.push(error),
        }
        match self.transport.read(SourceKind::FixedSshProbe).await {
            Ok(body) => match serde_json::from_slice::<Vec<RuntimeDto>>(&body) {
                Ok(values) => {
                    for value in values {
                        resources.push(
                            map_runtime(&request.scope, at, SourceKind::FixedSshProbe, value)
                                .map_err(|_| invalid_response())?,
                        );
                    }
                }
                Err(_) => failures.push(invalid_response()),
            },
            Err(error) => failures.push(error),
        }
        resources.sort_by_key(|resource| (resource.kind.clone(), resource.external_id.clone()));
        if resources.is_empty() && !failures.is_empty() {
            return Err(failures.remove(0));
        }
        let (coverage, outcome) = if failures.is_empty() {
            (
                next_infra_core::SyncCoverage::AuthoritativeFull {
                    scope: request.scope.clone(),
                },
                next_infra_connector_api::SyncOutcome::Complete {
                    batch: next_infra_connector_api::ObservationBatch {
                        resources,
                        relations: vec![],
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
        } else {
            (
                next_infra_core::SyncCoverage::Partial {
                    scope: Some(request.scope.clone()),
                    reason: next_infra_core::CoverageGapReason::ProviderUnavailable,
                },
                next_infra_connector_api::SyncOutcome::Partial {
                    batch: next_infra_connector_api::ObservationBatch {
                        resources,
                        relations: vec![],
                        coverage: next_infra_core::SyncCoverage::Partial {
                            scope: Some(request.scope.clone()),
                            reason: next_infra_core::CoverageGapReason::ProviderUnavailable,
                        },
                        next_cursor: None,
                        warnings: vec![],
                        redaction_report: Default::default(),
                        provider_request_summary: Default::default(),
                    },
                    failure: failures.remove(0),
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
    let service = ResourceKind::new("supabase.self_hosted.service").unwrap();
    let database = ResourceKind::new("supabase.self_hosted.database").unwrap();
    let runtime = ResourceKind::new("supabase.self_hosted.runtime").unwrap();
    next_infra_connector_api::ConnectorDescriptor { connector_type: ConnectorType::new("supabase-self-hosted").unwrap(), connector_version: "1.0.0".into(), config_schema_version: SchemaVersion::new(1).unwrap(), auth: next_infra_connector_api::AuthDescriptor { kind: next_infra_connector_api::AuthKind::Token, minimum_permissions: vec!["service API read".into(), "PostgreSQL metadata read".into(), "registered SSH probe".into()] }, sync_modes: vec![SyncMode::Full, SyncMode::Targeted], resources: vec![cap(service, "supabase.self_hosted.service_api", ConnectorCoverageLevel::Partial, "deployment exposes vary by installation"), cap(database, "supabase.self_hosted.postgres_metadata", ConnectorCoverageLevel::Partial, "metadata only; no data or connection strings"), cap(runtime, "supabase.self_hosted.runtime", ConnectorCoverageLevel::Partial, "fixed probe summary only")], relations: vec![], sensitive_field_policy: vec!["container environment, database config, credentials and arbitrary command output are excluded".into()], rate_limit: next_infra_connector_api::RateLimitGuidance { default_max_concurrency: 1, requests_per_minute: None, respects_retry_after: true }, recommended_sync_interval_secs: 900, known_gaps: vec!["This connector does not call the Supabase Management API".into()] }
}
fn cap(
    kind: ResourceKind,
    module: &str,
    level: ConnectorCoverageLevel,
    reason: &str,
) -> next_infra_connector_api::ResourceCapability {
    next_infra_connector_api::ResourceCapability {
        kind,
        attribute_schema_version: SchemaVersion::new(1).unwrap(),
        coverage: ConnectorCoverage {
            module: module.into(),
            level,
            reason: Some(reason.into()),
        },
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct ServiceDto {
    pub id: String,
    pub name: String,
    pub version: Option<String>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct DatabaseDto {
    pub id: String,
    pub engine: Option<String>,
    pub version: Option<String>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct RuntimeDto {
    pub id: String,
    pub platform: Option<String>,
    pub uptime_secs: Option<u64>,
}

pub fn map_service(
    scope: &Scope,
    at: Timestamp,
    source: SourceKind,
    v: ServiceDto,
) -> Result<ResourceObservation, String> {
    map(
        "supabase.self_hosted.service",
        &v.id,
        &v.name,
        source,
        scope,
        at,
        json!({"version": v.version}),
    )
}
pub fn map_database(
    scope: &Scope,
    at: Timestamp,
    source: SourceKind,
    v: DatabaseDto,
) -> Result<ResourceObservation, String> {
    map(
        "supabase.self_hosted.database",
        &v.id,
        &v.id,
        source,
        scope,
        at,
        json!({"engine": v.engine, "version": v.version}),
    )
}
pub fn map_runtime(
    scope: &Scope,
    at: Timestamp,
    source: SourceKind,
    v: RuntimeDto,
) -> Result<ResourceObservation, String> {
    map(
        "supabase.self_hosted.runtime",
        &v.id,
        &v.id,
        source,
        scope,
        at,
        json!({"platform": v.platform, "uptime_secs": v.uptime_secs}),
    )
}
fn map(
    kind: &str,
    id: &str,
    name: &str,
    source: SourceKind,
    scope: &Scope,
    at: Timestamp,
    mut attributes: serde_json::Value,
) -> Result<ResourceObservation, String> {
    if id.is_empty() {
        return Err("self-hosted identity is invalid".into());
    }
    attributes["source"] = json!(source.as_str());
    Ok(ResourceObservation {
        kind: ResourceKind::new(kind).map_err(|_| "invalid kind")?,
        external_id: ExternalId::new(format!("{kind}:{}:{id}", source.as_str()))
            .map_err(|_| "invalid id")?,
        name: id.into(),
        display_name: name.into(),
        scope: scope.clone(),
        labels: BTreeMap::from([
            (
                LabelKey::new("supabase.control_plane").unwrap(),
                "self_hosted".into(),
            ),
            (
                LabelKey::new("supabase.source").unwrap(),
                source.as_str().into(),
            ),
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
    use std::sync::Mutex;
    #[test]
    fn descriptor_declares_independent_sources() {
        let d = descriptor();
        assert!(d.validate().is_ok());
        assert!(check_descriptor(&d).is_empty());
        assert!(serde_json::to_string(&d).unwrap().contains("fixed probe"));
    }
    #[test]
    fn source_is_part_of_identity_and_environment_is_dropped() {
        let v: RuntimeDto =
            serde_json::from_str(r#"{"id":"runtime-1","platform":"linux","env":"secret"}"#)
                .unwrap();
        let o = map_runtime(
            &Scope::new("fixture-scope").unwrap(),
            Timestamp::from_unix_millis(1).unwrap(),
            SourceKind::FixedSshProbe,
            v,
        )
        .unwrap();
        let s = serde_json::to_string(&o).unwrap();
        assert!(s.contains("fixed_ssh_probe"));
        assert!(!s.contains("secret"));
    }

    struct FakeSelfHostedTransport {
        bodies: Mutex<
            std::collections::BTreeMap<
                &'static str,
                Result<Vec<u8>, next_infra_connector_api::ConnectorFailure>,
            >,
        >,
    }
    #[async_trait]
    impl SelfHostedTransport for FakeSelfHostedTransport {
        async fn read(
            &self,
            source: SourceKind,
        ) -> Result<Vec<u8>, next_infra_connector_api::ConnectorFailure> {
            self.bodies
                .lock()
                .unwrap()
                .remove(source.as_str())
                .unwrap_or_else(|| Err(invalid_response()))
        }
    }
    fn request() -> next_infra_connector_api::SyncRequest {
        next_infra_connector_api::SyncRequest {
            sync_run_id: next_infra_core::SyncRunId::new("supabase-self-hosted-fixture-run")
                .unwrap(),
            connection: next_infra_connector_api::ConnectionInput {
                connection_id: next_infra_core::ConnectionId::new(
                    "supabase-self-hosted-fixture-connection",
                )
                .unwrap(),
                connector_type: next_infra_core::ConnectorType::new("supabase-self-hosted")
                    .unwrap(),
                config: serde_json::json!({}),
                config_schema_version: next_infra_core::SchemaVersion::new(1).unwrap(),
            },
            mode: next_infra_core::SyncMode::Full,
            scope: next_infra_core::Scope::new("supabase-self-hosted-fixture-scope").unwrap(),
            cursor: None,
            targeted_resources: vec![],
        }
    }
    #[tokio::test]
    async fn connector_keeps_valid_sources_when_one_source_is_partial() {
        let mut bodies = std::collections::BTreeMap::new();
        bodies.insert(
            "service_api",
            Ok(br#"[{"id":"service-1","name":"Fixture API","version":"1"}]"#.to_vec()),
        );
        bodies.insert("postgres_metadata", Err(invalid_response()));
        bodies.insert(
            "fixed_ssh_probe",
            Ok(br#"[{"id":"runtime-1","platform":"linux","uptime_secs":1}]"#.to_vec()),
        );
        let connector = SupabaseSelfHostedConnector::new(FakeSelfHostedTransport {
            bodies: Mutex::new(bodies),
        });
        let outcome = connector
            .sync(
                request(),
                Some(&next_infra_core::SecretValue::new("fixture-token")),
            )
            .await
            .unwrap();
        let next_infra_connector_api::SyncOutcome::Partial { batch, .. } = outcome else {
            panic!("expected partial")
        };
        assert_eq!(batch.resources.len(), 2);
        assert!(matches!(
            batch.coverage,
            next_infra_core::SyncCoverage::Partial { .. }
        ));
    }
}
