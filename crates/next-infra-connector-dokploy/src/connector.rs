use crate::{
    ApplicationDto, DeploymentDto, DokployClient, DokployClientError, DokployTransport, DomainDto,
    ProjectDto, ServerDto, dokploy_descriptor, map_resources,
};
use async_trait::async_trait;
use next_infra_connector_api::*;
use next_infra_core::*;
use serde::de::DeserializeOwned;

pub struct DokployConnector<T> {
    descriptor: ConnectorDescriptor,
    client: DokployClient<T>,
}

impl<T> DokployConnector<T> {
    pub fn new(base_url: &str, transport: T) -> Result<Self, ConnectorFailure> {
        Ok(Self {
            descriptor: dokploy_descriptor(),
            client: DokployClient::new(base_url, transport).map_err(failure)?,
        })
    }
}

#[async_trait]
impl<T: DokployTransport> ReadConnector for DokployConnector<T> {
    fn descriptor(&self) -> &ConnectorDescriptor {
        &self.descriptor
    }

    async fn validate(
        &self,
        request: ValidationRequest,
        secret: Option<&SecretValue>,
    ) -> ConnectorResult<ValidationReport> {
        let mut errors = validate_connection(&request.connection);
        let Some(secret) = secret else {
            errors.push(issue(
                ErrorCode::CredentialUnavailable,
                "Dokploy credential is unavailable",
            ));
            return Ok(invalid_report(errors));
        };
        if !errors.is_empty() {
            return Ok(invalid_report(errors));
        }
        match self.client.fetch_pages("/api/projects", secret).await {
            Ok(_) => Ok(ValidationReport {
                status: ValidationStatus::Valid,
                warnings: Vec::new(),
                errors: Vec::new(),
            }),
            Err(error) => Ok(invalid_report(vec![issue(
                error.code,
                "Dokploy validation request failed",
            )])),
        }
    }

    async fn sync(
        &self,
        request: SyncRequest,
        secret: Option<&SecretValue>,
    ) -> ConnectorResult<SyncOutcome> {
        if let Some(issue) = validate_connection(&request.connection).into_iter().next() {
            return Err(ConnectorFailure {
                code: issue.code,
                message: issue.message,
                retryable: false,
                retry_after_ms: None,
            });
        }
        let secret = secret.ok_or_else(|| ConnectorFailure {
            code: ErrorCode::CredentialUnavailable,
            message: "Dokploy credential is unavailable".into(),
            retryable: false,
            retry_after_ms: None,
        })?;
        if request.mode != SyncMode::Full {
            return Err(ConnectorFailure {
                code: ErrorCode::InvalidDomainValue,
                message: "Dokploy supports full sync only in this release".into(),
                retryable: false,
                retry_after_ms: None,
            });
        }
        let projects: Vec<ProjectDto> = self.fetch("/api/projects", secret).await?;
        let applications: Vec<ApplicationDto> = self.fetch("/api/applications", secret).await?;
        let deployments: Vec<DeploymentDto> = self.fetch("/api/deployments", secret).await?;
        let servers: Vec<ServerDto> = self.fetch("/api/servers", secret).await?;
        let domains: Vec<DomainDto> = self.fetch("/api/domains", secret).await?;
        let mapped = map_resources(
            &request.scope,
            Timestamp::from_unix_millis(0).map_err(|_| internal())?,
            projects,
            applications,
            deployments,
            servers,
            domains,
        )
        .map_err(|_| invalid_response())?;
        let batch = ObservationBatch {
            resources: mapped.resources,
            relations: mapped.relations,
            coverage: SyncCoverage::AuthoritativeFull {
                scope: request.scope.clone(),
            },
            next_cursor: None,
            warnings: Vec::new(),
            redaction_report: RedactionReport {
                removed_fields: 0,
                unknown_fields_dropped: 0,
                secret_sentinels_detected: 0,
            },
            provider_request_summary: ProviderRequestSummary {
                request_count: 5,
                elapsed_ms: 0,
                status_class_counts: Default::default(),
            },
        };
        let outcome = SyncOutcome::Complete { batch };
        outcome
            .validate_for(&request)
            .map_err(|_| invalid_response())?;
        Ok(outcome)
    }
}

impl<T: DokployTransport> DokployConnector<T> {
    async fn fetch<V: DeserializeOwned>(
        &self,
        path: &str,
        secret: &SecretValue,
    ) -> ConnectorResult<Vec<V>> {
        let pages = self
            .client
            .fetch_pages(path, secret)
            .await
            .map_err(failure)?;
        let mut values = Vec::new();
        for page in pages {
            values.extend(serde_json::from_slice::<Vec<V>>(&page).map_err(|_| invalid_response())?);
        }
        Ok(values)
    }
}

fn validate_connection(connection: &ConnectionInput) -> Vec<ValidationIssue> {
    let mut errors = Vec::new();
    if connection.connector_type != ConnectorType::new("dokploy").expect("static connector") {
        errors.push(issue(
            ErrorCode::InvalidDomainValue,
            "Dokploy connection uses a different connector type",
        ));
    }
    if connection.config_schema_version != SchemaVersion::new(1).expect("static schema") {
        errors.push(issue(
            ErrorCode::SchemaIncompatible,
            "Dokploy connection config schema is unsupported",
        ));
    }
    if connection
        .config
        .get("base_url")
        .and_then(|value| value.as_str())
        .is_none()
    {
        errors.push(issue(
            ErrorCode::InvalidDomainValue,
            "Dokploy connection config requires a base_url",
        ));
    }
    errors
}
fn invalid_report(errors: Vec<ValidationIssue>) -> ValidationReport {
    ValidationReport {
        status: ValidationStatus::Invalid,
        warnings: Vec::new(),
        errors,
    }
}
fn issue(code: ErrorCode, message: &str) -> ValidationIssue {
    ValidationIssue {
        code,
        message: message.into(),
    }
}
fn failure(error: DokployClientError) -> ConnectorFailure {
    ConnectorFailure {
        code: error.code,
        message: "Dokploy transport request failed".into(),
        retryable: matches!(
            error.code,
            ErrorCode::RateLimited | ErrorCode::NetworkUnreachable | ErrorCode::ProviderUnavailable
        ),
        retry_after_ms: error.retry_after_ms,
    }
}
fn invalid_response() -> ConnectorFailure {
    ConnectorFailure {
        code: ErrorCode::InvalidResponse,
        message: "Dokploy response does not match the allowlisted DTO contract".into(),
        retryable: false,
        retry_after_ms: None,
    }
}
fn internal() -> ConnectorFailure {
    ConnectorFailure {
        code: ErrorCode::InvalidDomainValue,
        message: "Dokploy sync could not establish an observation timestamp".into(),
        retryable: false,
        retry_after_ms: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DokployResponse;
    use reqwest::StatusCode;
    use std::sync::Mutex;

    struct FakeTransport {
        responses: Mutex<Vec<Result<DokployResponse, DokployClientError>>>,
    }

    #[async_trait]
    impl DokployTransport for FakeTransport {
        async fn execute(
            &self,
            request: crate::DokployRequest,
        ) -> Result<DokployResponse, DokployClientError> {
            assert!(request.authorization.is_sensitive());
            self.responses.lock().unwrap().pop().unwrap()
        }
    }

    fn page(body: &str) -> Result<DokployResponse, DokployClientError> {
        Ok(DokployResponse {
            status: StatusCode::OK,
            retry_after_seconds: None,
            next_cursor: None,
            body: body.as_bytes().to_vec(),
        })
    }

    fn request() -> SyncRequest {
        SyncRequest {
            sync_run_id: SyncRunId::new("dokploy-fixture-run").unwrap(),
            connection: ConnectionInput {
                connection_id: ConnectionId::new("dokploy-fixture-connection").unwrap(),
                connector_type: ConnectorType::new("dokploy").unwrap(),
                config: serde_json::json!({"base_url":"https://dokploy.example.test"}),
                config_schema_version: SchemaVersion::new(1).unwrap(),
            },
            mode: SyncMode::Full,
            scope: Scope::new("dokploy-fixture-scope").unwrap(),
            cursor: None,
            targeted_resources: Vec::new(),
        }
    }

    #[tokio::test]
    async fn full_sync_collects_allowlisted_resources_and_provider_relations() {
        let connector = DokployConnector::new(
            "https://dokploy.example.test",
            FakeTransport {
                responses: Mutex::new(vec![
                    page(r#"[{"id":"domain","domain":"fixture.example.test","application_id":"application"}]"#),
                    page(r#"[{"id":"server","name":"Fixture server"}]"#),
                    page(r#"[{"id":"deployment","application_id":"application"}]"#),
                    page(r#"[{"id":"application","name":"Fixture application","project_id":"project","server_id":"server","password":"must-drop"}]"#),
                    page(r#"[{"id":"project","name":"Fixture project","token":"must-drop"}]"#),
                ]),
            },
        )
        .unwrap();
        let outcome = connector
            .sync(request(), Some(&SecretValue::new("fixture-token")))
            .await
            .unwrap();
        let SyncOutcome::Complete { batch } = outcome else {
            panic!("expected complete outcome")
        };
        assert_eq!(batch.resources.len(), 5);
        assert_eq!(batch.relations.len(), 4);
        let serialized = serde_json::to_string(&batch).unwrap();
        assert!(!serialized.contains("must-drop"));
    }
}
