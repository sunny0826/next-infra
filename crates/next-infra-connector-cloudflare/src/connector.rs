use crate::{
    AccountDto, CloudflareClient, CloudflareClientError, CloudflareTransport, DnsRecordDto,
    TunnelDto, TunnelWithAccount, WorkerDto, WorkerWithAccount, ZoneDto, cloudflare_descriptor,
    map_resources,
};
use async_trait::async_trait;
use next_infra_connector_api::*;
use next_infra_core::*;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;
use std::collections::HashMap;

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
        let observed_at = Timestamp::from_unix_millis(0).map_err(|_| invalid_response())?;
        let mut collector = Collector::new(request.scope.clone(), observed_at);

        // Root modules — failures are fatal (accounts, zones)
        let accounts: Vec<AccountDto> = self.fetch("/client/v4/accounts", secret).await?;
        let accounts_for_child = accounts.clone();
        collector.add_accounts(accounts);
        let zones: Vec<ZoneDto> = self.fetch("/client/v4/zones", secret).await?;
        let zones_for_child = zones.clone();
        collector.add_zones(zones);

        // Child modules — PermissionDenied, NotFound, RateLimited, or non-success
        // with retryable semantics are collected as partial; sync continues.
        let dns_records_result = self
            .fetch_child_zone_records(&zones_for_child, secret)
            .await;
        collector.merge_zone_records(dns_records_result);

        let tunnels_result = self.fetch_child_tunnels(&accounts_for_child, secret).await;
        collector.merge_tunnels(tunnels_result);

        let workers_result = self.fetch_child_workers(&accounts_for_child, secret).await;
        collector.merge_workers(workers_result);

        collector.finish(&request)
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

    async fn fetch_optional<V: DeserializeOwned>(
        &self,
        path: &str,
        secret: &SecretValue,
    ) -> Option<(Vec<V>, Option<ConnectorFailure>)> {
        match self.fetch(path, secret).await {
            Ok(values) => Some((values, None)),
            Err(failure) => {
                if is_child_module_fatal(&failure) {
                    None
                } else {
                    Some((Vec::new(), Some(failure)))
                }
            }
        }
    }

    async fn fetch_child_zone_records(
        &self,
        zones: &[ZoneDto],
        secret: &SecretValue,
    ) -> HashMap<String, (Vec<DnsRecordDto>, Option<ConnectorFailure>)> {
        let mut results = HashMap::new();
        for zone in zones {
            let result = self
                .fetch_optional::<DnsRecordDto>(
                    &format!("/client/v4/zones/{}/dns_records", zone.id),
                    secret,
                )
                .await;
            if let Some((records, failure)) = result {
                results.insert(zone.id.clone(), (records, failure));
            }
        }
        results
    }

    async fn fetch_child_tunnels(
        &self,
        accounts: &[AccountDto],
        secret: &SecretValue,
    ) -> HashMap<String, (Vec<TunnelWithAccount>, Option<ConnectorFailure>)> {
        let mut results = HashMap::new();
        for account in accounts {
            let result = self
                .fetch_optional::<TunnelDto>(
                    &format!("/client/v4/accounts/{}/cfd_tunnel", account.id),
                    secret,
                )
                .await;
            if let Some((tunnels, failure)) = result {
                let with_account = tunnels
                    .into_iter()
                    .map(|tunnel| TunnelWithAccount {
                        tunnel,
                        account_id: account.id.clone(),
                    })
                    .collect();
                results.insert(account.id.clone(), (with_account, failure));
            }
        }
        results
    }

    async fn fetch_child_workers(
        &self,
        accounts: &[AccountDto],
        secret: &SecretValue,
    ) -> HashMap<String, (Vec<WorkerWithAccount>, Option<ConnectorFailure>)> {
        let mut results = HashMap::new();
        for account in accounts {
            let result = self
                .fetch_optional::<WorkerDto>(
                    &format!("/client/v4/accounts/{}/workers/scripts", account.id),
                    secret,
                )
                .await;
            if let Some((workers, failure)) = result {
                let with_account = workers
                    .into_iter()
                    .map(|worker| WorkerWithAccount {
                        worker,
                        account_id: account.id.clone(),
                    })
                    .collect();
                results.insert(account.id.clone(), (with_account, failure));
            }
        }
        results
    }
}

struct Collector {
    scope: Scope,
    observed_at: Timestamp,
    accounts: Vec<AccountDto>,
    zones: Vec<ZoneDto>,
    records: Vec<DnsRecordDto>,
    tunnels: Vec<TunnelWithAccount>,
    workers: Vec<WorkerWithAccount>,
    warnings: Vec<ObservationWarning>,
    status_counts: BTreeMap<String, u64>,
    request_count: u64,
    child_failures: Vec<ConnectorFailure>,
}

impl Collector {
    fn new(scope: Scope, observed_at: Timestamp) -> Self {
        Self {
            scope,
            observed_at,
            accounts: Vec::new(),
            zones: Vec::new(),
            records: Vec::new(),
            tunnels: Vec::new(),
            workers: Vec::new(),
            warnings: Vec::new(),
            status_counts: BTreeMap::new(),
            request_count: 0,
            child_failures: Vec::new(),
        }
    }

    fn add_accounts(&mut self, accounts: Vec<AccountDto>) {
        self.accounts = accounts;
    }

    fn add_zones(&mut self, zones: Vec<ZoneDto>) {
        self.zones = zones;
    }

    fn merge_zone_records(
        &mut self,
        results: HashMap<String, (Vec<DnsRecordDto>, Option<ConnectorFailure>)>,
    ) {
        for (zone_id, (mut recs, failure)) in results {
            self.request_count = self.request_count.saturating_add(1);
            if let Some(ref f) = failure {
                *self.status_counts.entry(status_key(f.code)).or_insert(0) += 1;
                self.child_failures.push(f.clone());
                self.warnings.push(ObservationWarning {
                    code: f.code,
                    message: format!("Cloudflare module dns_records for zone {zone_id} is partial"),
                });
            } else {
                *self.status_counts.entry("2xx".to_string()).or_insert(0) += 1;
            }
            self.records.append(&mut recs);
        }
    }

    fn merge_tunnels(
        &mut self,
        results: HashMap<String, (Vec<TunnelWithAccount>, Option<ConnectorFailure>)>,
    ) {
        for (account_id, (mut tuns, failure)) in results {
            self.request_count = self.request_count.saturating_add(1);
            if let Some(ref f) = failure {
                *self.status_counts.entry(status_key(f.code)).or_insert(0) += 1;
                self.child_failures.push(f.clone());
                self.warnings.push(ObservationWarning {
                    code: f.code,
                    message: format!(
                        "Cloudflare module tunnels for account {account_id} is partial"
                    ),
                });
            } else {
                *self.status_counts.entry("2xx".to_string()).or_insert(0) += 1;
            }
            self.tunnels.append(&mut tuns);
        }
    }

    fn merge_workers(
        &mut self,
        results: HashMap<String, (Vec<WorkerWithAccount>, Option<ConnectorFailure>)>,
    ) {
        for (account_id, (mut wks, failure)) in results {
            self.request_count = self.request_count.saturating_add(1);
            if let Some(ref f) = failure {
                *self.status_counts.entry(status_key(f.code)).or_insert(0) += 1;
                self.child_failures.push(f.clone());
                self.warnings.push(ObservationWarning {
                    code: f.code,
                    message: format!(
                        "Cloudflare module workers for account {account_id} is partial"
                    ),
                });
            } else {
                *self.status_counts.entry("2xx".to_string()).or_insert(0) += 1;
            }
            self.workers.append(&mut wks);
        }
    }

    fn finish(self, request: &SyncRequest) -> ConnectorResult<SyncOutcome> {
        let mapped = map_resources(
            &self.scope,
            self.observed_at,
            self.accounts,
            self.zones,
            self.records,
            self.tunnels,
            self.workers,
        )
        .map_err(|_| invalid_response())?;

        let primary_failure = self.child_failures.first().cloned();
        let coverage = match &primary_failure {
            Some(failure) => SyncCoverage::Partial {
                scope: Some(self.scope.clone()),
                reason: coverage_reason(failure.code),
            },
            None => SyncCoverage::AuthoritativeFull {
                scope: self.scope.clone(),
            },
        };

        let batch = ObservationBatch {
            resources: mapped.resources,
            relations: mapped.relations,
            coverage,
            next_cursor: None,
            warnings: self.warnings,
            redaction_report: RedactionReport::default(),
            provider_request_summary: ProviderRequestSummary {
                request_count: self.request_count,
                elapsed_ms: 0,
                status_class_counts: self.status_counts,
            },
        };

        let outcome = match &primary_failure {
            Some(failure) => SyncOutcome::Partial {
                batch,
                failure: failure.clone(),
            },
            None => SyncOutcome::Complete { batch },
        };
        outcome
            .validate_for(request)
            .map_err(|_| invalid_response())?;
        Ok(outcome)
    }
}

fn is_child_module_fatal(failure: &ConnectorFailure) -> bool {
    matches!(
        failure.code,
        ErrorCode::AuthenticationFailed | ErrorCode::CredentialUnavailable
    )
}

fn status_key(code: ErrorCode) -> String {
    match code {
        ErrorCode::AuthenticationFailed => "401".to_string(),
        ErrorCode::PermissionDenied => "403".to_string(),
        ErrorCode::RateLimited => "429".to_string(),
        ErrorCode::NetworkUnreachable
        | ErrorCode::ProviderUnavailable
        | ErrorCode::PartialPagination
        | ErrorCode::InvalidResponse
        | ErrorCode::SchemaIncompatible
        | ErrorCode::InvalidDomainValue
        | ErrorCode::CredentialUnavailable => "5xx".to_string(),
        _ => "unknown".to_string(),
    }
}

fn coverage_reason(code: ErrorCode) -> CoverageGapReason {
    match code {
        ErrorCode::PermissionDenied => CoverageGapReason::PermissionDenied,
        ErrorCode::RateLimited => CoverageGapReason::RateLimited,
        ErrorCode::ProviderUnavailable | ErrorCode::NetworkUnreachable => {
            CoverageGapReason::ProviderUnavailable
        }
        ErrorCode::SchemaIncompatible | ErrorCode::InvalidResponse => {
            CoverageGapReason::SchemaIncompatible
        }
        _ => CoverageGapReason::PaginationIncomplete,
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
                page(r#"[{"id":"worker","modified_on":"now","script":"must-drop"}]"#),
                page(r#"[{"id":"tunnel","name":"Fixture tunnel"}]"#),
                page(
                    r#"[{"id":"record","zone_id":"zone","type":"A","name":"fixture.example.test","content":"192.0.2.1"}]"#,
                ),
                page(
                    r#"[{"id":"zone","name":"fixture.example.test","account":{"id":"account","name":"Fixture account"}}]"#,
                ),
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

    fn forbidden() -> Result<CloudflareResponse, CloudflareClientError> {
        Ok(CloudflareResponse {
            status: StatusCode::FORBIDDEN,
            retry_after_seconds: None,
            body: Vec::new(),
        })
    }

    fn rate_limited() -> Result<CloudflareResponse, CloudflareClientError> {
        Ok(CloudflareResponse {
            status: StatusCode::TOO_MANY_REQUESTS,
            retry_after_seconds: Some(30),
            body: Vec::new(),
        })
    }

    #[tokio::test]
    async fn child_permission_failure_is_partial_and_other_modules_survive() {
        let connector = CloudflareConnector::new(FakeTransport {
            responses: Mutex::new(vec![
                page(r#"[{"id":"worker","modified_on":"now"}]"#),
                page(r#"[{"id":"tunnel","name":"Fixture tunnel"}]"#),
                forbidden(),
                page(
                    r#"[{"id":"zone","name":"fixture.example.test","account":{"id":"account","name":"Fixture account"}}]"#,
                ),
                page(r#"[{"id":"account","name":"Fixture account"}]"#),
            ]),
        });
        let outcome = connector
            .sync(request(), Some(&SecretValue::new("fixture-token")))
            .await
            .unwrap();
        let SyncOutcome::Partial { batch, failure } = outcome else {
            panic!("expected partial outcome")
        };
        assert_eq!(failure.code, ErrorCode::PermissionDenied);
        assert!(batch.warnings.iter().any(|w| {
            w.code == ErrorCode::PermissionDenied
                && w.message == "Cloudflare module dns_records for zone zone is partial"
        }));
        assert_eq!(batch.resources.len(), 4);
        assert_eq!(batch.relations.len(), 3);
    }

    #[tokio::test]
    async fn child_rate_limit_is_partial_and_other_modules_survive() {
        let connector = CloudflareConnector::new(FakeTransport {
            responses: Mutex::new(vec![
                page(r#"[{"id":"worker","modified_on":"now"}]"#),
                rate_limited(),
                page(
                    r#"[{"id":"record","zone_id":"zone","type":"A","name":"fixture.example.test","content":"192.0.2.1"}]"#,
                ),
                page(
                    r#"[{"id":"zone","name":"fixture.example.test","account":{"id":"account","name":"Fixture account"}}]"#,
                ),
                page(r#"[{"id":"account","name":"Fixture account"}]"#),
            ]),
        });
        let outcome = connector
            .sync(request(), Some(&SecretValue::new("fixture-token")))
            .await
            .unwrap();
        let SyncOutcome::Partial { batch, failure } = outcome else {
            panic!("expected partial outcome")
        };
        assert_eq!(failure.code, ErrorCode::RateLimited);
        assert!(batch.warnings.iter().any(|w| {
            w.code == ErrorCode::RateLimited
                && w.message == "Cloudflare module tunnels for account account is partial"
        }));
        assert_eq!(batch.resources.len(), 4);
        assert_eq!(batch.relations.len(), 3);
    }

    #[tokio::test]
    async fn child_not_found_is_partial_and_other_modules_survive() {
        let connector = CloudflareConnector::new(FakeTransport {
            responses: Mutex::new(vec![
                Ok(CloudflareResponse {
                    status: StatusCode::NOT_FOUND,
                    retry_after_seconds: None,
                    body: Vec::new(),
                }),
                page(r#"[{"id":"tunnel","name":"Fixture tunnel"}]"#),
                page(
                    r#"[{"id":"record","zone_id":"zone","type":"A","name":"fixture.example.test","content":"192.0.2.1"}]"#,
                ),
                page(
                    r#"[{"id":"zone","name":"fixture.example.test","account":{"id":"account","name":"Fixture account"}}]"#,
                ),
                page(r#"[{"id":"account","name":"Fixture account"}]"#),
            ]),
        });
        let outcome = connector
            .sync(request(), Some(&SecretValue::new("fixture-token")))
            .await
            .unwrap();
        let SyncOutcome::Partial { batch, failure } = outcome else {
            panic!("expected partial outcome")
        };
        assert_eq!(failure.code, ErrorCode::ProviderUnavailable);
        assert_eq!(batch.resources.len(), 4);
        assert_eq!(batch.relations.len(), 3);
    }

    #[tokio::test]
    async fn auth_failure_on_root_module_is_fatal() {
        let connector = CloudflareConnector::new(FakeTransport {
            responses: Mutex::new(vec![Ok(CloudflareResponse {
                status: StatusCode::UNAUTHORIZED,
                retry_after_seconds: None,
                body: Vec::new(),
            })]),
        });
        let failure = connector
            .sync(request(), Some(&SecretValue::new("fixture-token")))
            .await
            .unwrap_err();
        assert_eq!(failure.code, ErrorCode::AuthenticationFailed);
    }
}
