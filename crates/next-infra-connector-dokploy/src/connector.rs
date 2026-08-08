use crate::{
    ApplicationDto, DeploymentDto, DokployClient, DokployClientError, DokployTransport, DomainDto,
    ProjectDto, ServerDto, dokploy_descriptor, map_resources,
};
use async_trait::async_trait;
use next_infra_connector_api::*;
use next_infra_core::*;
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;

/// Maximum applications to walk (per-app 2 requests: deployment.all + domain.byApplicationId)
const MAX_APPLICATIONS_WALK: usize = 50;

/// Maximum total requests per sync (project.all + server.all + per-app walks)
#[allow(dead_code)]
const MAX_REQUESTS_PER_SYNC: u64 = 200;

#[cfg(test)]
const TEST_MAX_REQUESTS: u64 = 5;

#[cfg(test)]
fn request_budget() -> u64 {
    TEST_MAX_REQUESTS
}

#[cfg(not(test))]
fn request_budget() -> u64 {
    MAX_REQUESTS_PER_SYNC
}

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
        match self.client.fetch_pages("/api/project.all", secret).await {
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

        // Phase 1: fetch projects and servers (2 fixed requests)
        let mut request_count: u64 = 0;
        let projects: Vec<ProjectDto> = {
            let pages = self
                .fetch_pages_raw("/api/project.all", secret)
                .await
                .map_err(failure)?;
            request_count += 1;
            self.parse_pages(pages)?
        };
        let servers: Vec<ServerDto> = {
            let pages = self
                .fetch_pages_raw("/api/server.all", secret)
                .await
                .map_err(failure)?;
            request_count += 1;
            self.parse_pages(pages)?
        };

        // Phase 2: flatten applications from both old-shape (top-level) and new-shape
        // (environments[].applications), filling project_id from parent
        let mut flat_applications: Vec<ApplicationDto> = Vec::new();
        for project in &projects {
            for mut app in project.applications.clone() {
                app.project_id = Some(project.id.clone());
                flat_applications.push(app);
            }
            for env in &project.environments {
                for mut app in env.applications.clone() {
                    app.project_id = Some(project.id.clone());
                    flat_applications.push(app);
                }
            }
        }

        // Phase 3: per-application walk with request budget
        let mut deployments: Vec<DeploymentDto> = Vec::new();
        let mut domains: Vec<DomainDto> = Vec::new();
        let mut warnings: Vec<ObservationWarning> = Vec::new();
        let mut status_class_counts: BTreeMap<String, u64> = BTreeMap::new();
        let mut primary_failure: Option<ConnectorFailure> = None;

        *status_class_counts.entry("project".into()).or_insert(0) += projects.len() as u64;
        *status_class_counts.entry("server".into()).or_insert(0) += servers.len() as u64;

        let apps_to_walk = flat_applications
            .iter()
            .take(MAX_APPLICATIONS_WALK)
            .collect::<Vec<_>>();

        for app in apps_to_walk {
            if request_count >= request_budget() {
                let budget_failure = ConnectorFailure {
                    code: ErrorCode::PartialPagination,
                    message: "Dokploy request budget was exhausted".into(),
                    retryable: true,
                    retry_after_ms: None,
                };
                primary_failure.get_or_insert_with(|| budget_failure.clone());
                warnings.push(ObservationWarning {
                    code: ErrorCode::PartialPagination,
                    message: "Dokploy request budget was exhausted".into(),
                });
                break;
            }
            let app_id = &app.id;

            let dep_pages = self
                .fetch_pages_raw(
                    &format!("/api/deployment.all?applicationId={}", app_id),
                    secret,
                )
                .await
                .map_err(failure)?;
            request_count += 1;
            let app_deployments: Vec<DeploymentDto> = self.parse_pages(dep_pages)?;
            *status_class_counts.entry("deployment".into()).or_insert(0) +=
                app_deployments.len() as u64;
            deployments.extend(app_deployments);

            if request_count >= request_budget() {
                let budget_failure = ConnectorFailure {
                    code: ErrorCode::PartialPagination,
                    message: "Dokploy request budget was exhausted".into(),
                    retryable: true,
                    retry_after_ms: None,
                };
                primary_failure.get_or_insert_with(|| budget_failure.clone());
                warnings.push(ObservationWarning {
                    code: ErrorCode::PartialPagination,
                    message: "Dokploy request budget was exhausted".into(),
                });
                break;
            }

            let dom_pages = self
                .fetch_pages_raw(
                    &format!("/api/domain.byApplicationId?applicationId={}", app_id),
                    secret,
                )
                .await
                .map_err(failure)?;
            request_count += 1;
            let app_domains: Vec<DomainDto> = self.parse_pages(dom_pages)?;
            *status_class_counts.entry("domain".into()).or_insert(0) += app_domains.len() as u64;
            domains.extend(app_domains);
        }

        let observed_at = Timestamp::from_unix_millis(0).map_err(|_| internal())?;
        let mapped = map_resources(
            &request.scope,
            observed_at,
            projects,
            flat_applications,
            deployments,
            servers,
            domains,
        )
        .map_err(|_| invalid_response())?;

        let coverage = if primary_failure.is_none() {
            SyncCoverage::AuthoritativeFull {
                scope: request.scope.clone(),
            }
        } else {
            SyncCoverage::Partial {
                scope: Some(request.scope.clone()),
                reason: CoverageGapReason::PaginationIncomplete,
            }
        };

        let batch = ObservationBatch {
            resources: mapped.resources,
            relations: mapped.relations,
            coverage,
            next_cursor: None,
            warnings,
            redaction_report: RedactionReport {
                removed_fields: 0,
                unknown_fields_dropped: 0,
                secret_sentinels_detected: 0,
            },
            provider_request_summary: ProviderRequestSummary {
                request_count,
                elapsed_ms: 0,
                status_class_counts,
            },
        };

        let outcome = match primary_failure {
            Some(failure) => SyncOutcome::Partial { batch, failure },
            None => SyncOutcome::Complete { batch },
        };
        outcome
            .validate_for(&request)
            .map_err(|_| invalid_response())?;
        Ok(outcome)
    }
}

impl<T: DokployTransport> DokployConnector<T> {
    /// Fetch raw pages without parsing (used to count requests before we know the type)
    async fn fetch_pages_raw(
        &self,
        path: &str,
        secret: &SecretValue,
    ) -> Result<Vec<Vec<u8>>, DokployClientError> {
        self.client.fetch_pages(path, secret).await
    }

    /// Parse a sequence of pages into a Vec of T
    fn parse_pages<V: DeserializeOwned>(&self, pages: Vec<Vec<u8>>) -> ConnectorResult<Vec<V>> {
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

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
                config: serde_json::json!({"base_url": "https://dokploy.example.test"}),
                config_schema_version: SchemaVersion::new(1).unwrap(),
            },
            mode: SyncMode::Full,
            scope: Scope::new("dokploy-fixture-scope").unwrap(),
            cursor: None,
            targeted_resources: Vec::new(),
        }
    }

    #[tokio::test]
    async fn full_sync_collects_v2_resources_and_provider_relations() {
        // Responses are popped in reverse order (LIFO stack)
        let connector = DokployConnector::new(
            "https://dokploy.example.test",
            FakeTransport {
                responses: Mutex::new(vec![
                    // domain.byApplicationId?applicationId=app-1
                    page(r#"[{"domainId":"domain","host":"fixture.example.test","applicationId":"app-1"}]"#),
                    // deployment.all?applicationId=app-1
                    page(r#"[{"deploymentId":"deployment","applicationId":"app-1","status":"running"}]"#),
                    // server.all
                    page(r#"[{"serverId":"server","name":"Fixture server","ipAddress":"10.0.0.1"}]"#),
                    // project.all (v2 shape with environments nesting)
                    page(r#"[
                        {
                            "projectId": "project",
                            "name": "Fixture project",
                            "token": "must-drop",
                            "environments": [{
                                "environmentId": "env-1",
                                "name": "Production",
                                "applications": [{
                                    "applicationId": "app-1",
                                    "name": "Fixture application",
                                    "serverId": "server",
                                    "password": "must-drop"
                                }],
                                "compose": []
                            }]
                        }
                    ]"#),
                ]),
            },
        )
        .unwrap();
        let outcome = connector
            .sync(request(), Some(&SecretValue::new("fixture-token")))
            .await
            .unwrap();

        let SyncOutcome::Complete { batch } = outcome else {
            panic!("expected complete outcome");
        };
        // project(1) + application(1) + deployment(1) + server(1) + domain(1) = 5 resources
        assert_eq!(batch.resources.len(), 5);
        // contains(1) + runs_on(1) + deploys(1) + exposes(1) = 4 relations
        assert_eq!(batch.relations.len(), 4);
        let serialized = serde_json::to_string(&batch).unwrap();
        assert!(!serialized.contains("must-drop"));
        // Request count: project.all(1) + server.all(1) + deployment.all(1) + domain.byApplicationId(1) = 4
        assert_eq!(batch.provider_request_summary.request_count, 4);
    }

    #[tokio::test]
    async fn budget_exhaustion_produces_partial_outcome() {
        // With TEST_MAX_REQUESTS=5, after 5 requests the budget is exhausted.
        // Provide 6 responses (2 apps = 6 requests) — the 6th request is blocked
        // by the budget check before it reaches the transport.
        let connector = DokployConnector::new(
            "https://dokploy.example.test",
            FakeTransport {
                responses: Mutex::new(vec![
                    page(r#"[]"#), // app-2 domain (would be request 6 — budget blocks)
                    page(r#"[]"#), // app-2 deployment (would be request 6 — budget blocks)
                    page(r#"[{"domainId":"dom-1","host":"app1.example.test","applicationId":"app-1"}]"#), // app-1 domain (request 5)
                    page(r#"[{"deploymentId":"deploy-1","applicationId":"app-1"}]"#), // app-1 deployment (request 4)
                    page(r#"[{"serverId":"server","name":"Srv"}]"#), // server.all (request 2)
                    page(r#"[
                        {"projectId":"project","name":"Proj","environments":[
                            {"environmentId":"env-1","name":"Prod","applications":[
                                {"applicationId":"app-1","name":"App1","serverId":"server"},
                                {"applicationId":"app-2","name":"App2","serverId":"server"}
                            ]}
                        ]}
                    ]"#), // project.all (request 1)
                ]),
            },
        )
        .unwrap();
        let outcome = connector
            .sync(request(), Some(&SecretValue::new("fixture-token")))
            .await
            .unwrap();

        let SyncOutcome::Partial { batch, failure: _ } = outcome else {
            panic!("expected partial outcome due to budget exhaustion");
        };
        assert!(
            batch
                .warnings
                .iter()
                .any(|w| w.message.contains("request budget was exhausted"))
        );
        // request_count = project.all(1) + server.all(1) + app-1 deploy(1) + app-1 domain(1) + app-2 deploy(1) = 5
        assert_eq!(batch.provider_request_summary.request_count, 5);
    }

    #[tokio::test]
    async fn old_shape_applications_also_flattened() {
        // project with top-level applications (pre-v2 shape)
        let connector = DokployConnector::new(
            "https://dokploy.example.test",
            FakeTransport {
                responses: Mutex::new(vec![
                    page(r#"[{"domainId":"domain","host":"old.example.test","applicationId":"app-old"}]"#),
                    page(r#"[{"deploymentId":"deploy-old","applicationId":"app-old"}]"#),
                    page(r#"[{"serverId":"server","name":"Srv"}]"#),
                    page(r#"[
                        {
                            "projectId": "project-old",
                            "name": "Old Shape Project",
                            "applications": [{
                                "applicationId": "app-old",
                                "name": "Old App",
                                "serverId": "server"
                            }]
                        }
                    ]"#),
                ]),
            },
        )
        .unwrap();
        let outcome = connector
            .sync(request(), Some(&SecretValue::new("fixture-token")))
            .await
            .unwrap();

        let SyncOutcome::Complete { batch } = outcome else {
            panic!("expected complete");
        };
        assert_eq!(batch.resources.len(), 5);
        assert_eq!(batch.relations.len(), 4);
        // Verify the old-shape application got project_id filled
        let app_res = batch
            .resources
            .iter()
            .find(|r| r.kind.as_str() == "dokploy.application")
            .expect("application resource");
        assert_eq!(
            app_res
                .attributes
                .get("project_id")
                .and_then(|v| v.as_str()),
            Some("project-old")
        );
    }
}
