use crate::{
    AccountDto, CloudflareClient, CloudflareClientError, CloudflareTransport, DnsRecordDto,
    TunnelDto, WorkerDto, ZoneDto, cloudflare_descriptor, map_resources,
};
use async_trait::async_trait;
use next_infra_connector_api::*;
use next_infra_core::*;
use serde::Deserialize;
use serde::de::DeserializeOwned;

pub struct CloudflareConnector<T> {
    descriptor: ConnectorDescriptor,
    client: CloudflareClient<T>,
}

impl<T> CloudflareConnector<T> {
    pub fn new(transport: T) -> Self {
        Self {
            descriptor: cloudflare_descriptor(),
            client: CloudflareClient::new(transport),
        }
    }
}

#[async_trait]
impl<T: CloudflareTransport> ReadConnector for CloudflareConnector<T> {
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
                "Cloudflare credential is unavailable",
            ));
            return Ok(invalid_report(errors));
        };
        if !errors.is_empty() {
            return Ok(invalid_report(errors));
        }
        match self
            .fetch::<AccountDto>("/client/v4/accounts", secret)
            .await
        {
            Ok(_) => Ok(ValidationReport {
                status: ValidationStatus::Valid,
                warnings: Vec::new(),
                errors: Vec::new(),
            }),
            Err(error) => Ok(invalid_report(vec![issue(
                error.code,
                "Cloudflare validation request failed",
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
            message: "Cloudflare credential is unavailable".into(),
            retryable: false,
            retry_after_ms: None,
        })?;
        if request.mode != SyncMode::Full {
            return Err(ConnectorFailure {
                code: ErrorCode::InvalidDomainValue,
                message: "Cloudflare supports full sync only in this release".into(),
                retryable: false,
                retry_after_ms: None,
            });
        }
        let accounts: Vec<AccountDto> = self.fetch("/client/v4/accounts", secret).await?;
        let zones: Vec<ZoneDto> = self.fetch("/client/v4/zones", secret).await?;
        let mut records = Vec::new();
        for zone in &zones {
            records.extend(
                self.fetch::<DnsRecordDto>(
                    &format!("/client/v4/zones/{}/dns_records", zone.id),
                    secret,
                )
                .await?,
            );
        }
        let mut tunnels = Vec::new();
        let mut workers = Vec::new();
        for account in &accounts {
            tunnels.extend(
                self.fetch::<TunnelDto>(
                    &format!("/client/v4/accounts/{}/cfd_tunnel", account.id),
                    secret,
                )
                .await?,
            );
            workers.extend(
                self.fetch::<WorkerDto>(
                    &format!("/client/v4/accounts/{}/workers/scripts", account.id),
                    secret,
                )
                .await?,
            );
        }
        let mapped = map_resources(
            &request.scope,
            Timestamp::from_unix_millis(0).map_err(|_| invalid_response())?,
            accounts,
            zones,
            records,
            tunnels,
            workers,
        )
        .map_err(|_| invalid_response())?;
        let outcome = SyncOutcome::Complete {
            batch: ObservationBatch {
                resources: mapped.resources,
                relations: mapped.relations,
                coverage: SyncCoverage::AuthoritativeFull {
                    scope: request.scope.clone(),
                },
                next_cursor: None,
                warnings: Vec::new(),
                redaction_report: RedactionReport::default(),
                provider_request_summary: ProviderRequestSummary {
                    request_count: 2,
                    elapsed_ms: 0,
                    status_class_counts: Default::default(),
                },
            },
        };
        outcome
            .validate_for(&request)
            .map_err(|_| invalid_response())?;
        Ok(outcome)
    }
}

impl<T: CloudflareTransport> CloudflareConnector<T> {
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
        let mut output = Vec::new();
        for page in pages {
            output.extend(
                serde_json::from_slice::<ResultEnvelope<V>>(&page)
                    .map_err(|_| invalid_response())?
                    .result,
            );
        }
        Ok(output)
    }
}

#[derive(Deserialize)]
struct ResultEnvelope<T> {
    result: Vec<T>,
}

fn validate_connection(connection: &ConnectionInput) -> Vec<ValidationIssue> {
    let mut errors = Vec::new();
    if connection.connector_type != ConnectorType::new("cloudflare").expect("static connector") {
        errors.push(issue(
            ErrorCode::InvalidDomainValue,
            "Cloudflare connection uses a different connector type",
        ));
    }
    if connection.config_schema_version != SchemaVersion::new(1).expect("static schema") {
        errors.push(issue(
            ErrorCode::SchemaIncompatible,
            "Cloudflare connection config schema is unsupported",
        ));
    }
    if !connection
        .config
        .as_object()
        .is_some_and(serde_json::Map::is_empty)
    {
        errors.push(issue(
            ErrorCode::InvalidDomainValue,
            "Cloudflare connection config must be an empty object",
        ));
    }
    errors
}
fn issue(code: ErrorCode, message: &str) -> ValidationIssue {
    ValidationIssue {
        code,
        message: message.into(),
    }
}
fn invalid_report(errors: Vec<ValidationIssue>) -> ValidationReport {
    ValidationReport {
        status: ValidationStatus::Invalid,
        warnings: Vec::new(),
        errors,
    }
}
fn failure(error: CloudflareClientError) -> ConnectorFailure {
    ConnectorFailure {
        code: error.code,
        message: "Cloudflare transport request failed".into(),
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
        message: "Cloudflare response does not match the allowlisted DTO contract".into(),
        retryable: false,
        retry_after_ms: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CloudflareResponse;
    use reqwest::StatusCode;
    use std::sync::Mutex;

    struct FakeTransport {
        responses: Mutex<Vec<Result<CloudflareResponse, CloudflareClientError>>>,
    }
    #[async_trait]
    impl CloudflareTransport for FakeTransport {
        async fn execute(
            &self,
            request: crate::CloudflareRequest,
        ) -> Result<CloudflareResponse, CloudflareClientError> {
            assert!(request.authorization.is_sensitive());
            self.responses.lock().unwrap().pop().unwrap()
        }
    }
    fn page(result: &str) -> Result<CloudflareResponse, CloudflareClientError> {
        Ok(CloudflareResponse {
            status: StatusCode::OK,
            retry_after_seconds: None,
            body: format!(r#"{{"result":{result},"result_info":{{"total_pages":1}}}}"#)
                .into_bytes(),
        })
    }
    fn request() -> SyncRequest {
        SyncRequest {
            sync_run_id: SyncRunId::new("cloudflare-fixture-run").unwrap(),
            connection: ConnectionInput {
                connection_id: ConnectionId::new("cloudflare-fixture-connection").unwrap(),
                connector_type: ConnectorType::new("cloudflare").unwrap(),
                config: serde_json::json!({}),
                config_schema_version: SchemaVersion::new(1).unwrap(),
            },
            mode: SyncMode::Full,
            scope: Scope::new("cloudflare-fixture-scope").unwrap(),
            cursor: None,
            targeted_resources: Vec::new(),
        }
    }
    #[tokio::test]
    async fn full_sync_collects_allowlisted_summaries_without_worker_code() {
        let connector = CloudflareConnector::new(FakeTransport {
            responses: Mutex::new(vec![
                page(
                    r#"[{"id":"worker","account_id":"account","modified_on":"now","script":"must-drop"}]"#,
                ),
                page(r#"[{"id":"tunnel","account_id":"account","name":"Fixture tunnel"}]"#),
                page(
                    r#"[{"id":"record","zone_id":"zone","record_type":"A","name":"fixture.example.test","content":"192.0.2.1"}]"#,
                ),
                page(r#"[{"id":"zone","name":"fixture.example.test","account_id":"account"}]"#),
                page(r#"[{"id":"account","name":"Fixture account"}]"#),
            ]),
        });
        let outcome = connector
            .sync(request(), Some(&SecretValue::new("fixture-token")))
            .await
            .unwrap();
        let SyncOutcome::Complete { batch } = outcome else {
            panic!("expected complete outcome")
        };
        assert_eq!(batch.resources.len(), 5);
        assert_eq!(batch.relations.len(), 4);
        assert!(!serde_json::to_string(&batch).unwrap().contains("must-drop"));
    }
}
