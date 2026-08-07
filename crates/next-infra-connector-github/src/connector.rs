use crate::{
    GitHubClient, GitHubClock, GitHubEndpoint, GitHubFetch, GitHubFetchBudget, GitHubPage,
    GitHubPages, GitHubPaginationFailure, GitHubTransport, MAX_REQUESTS_PER_BATCH,
    actions::{
        ActionMapperOutput, GitHubRepositoryContext, JobListDto, WorkflowListDto,
        WorkflowRunListDto, map_jobs, map_runs, map_workflows,
    },
    deployment::{DeploymentDto, map_deployments},
    environment::{EnvironmentListDto, map_environments},
    github_descriptor,
    repository::{
        RepositoryDto, RepositoryMapperOutput, RepositoryRouteContext, find_targeted_route,
        map_repositories,
    },
};
use async_trait::async_trait;
use next_infra_connector_api::*;
use next_infra_core::*;
use serde::de::DeserializeOwned;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Mutex,
};

#[derive(Clone)]
struct CachedPages {
    etag: Option<String>,
    pages: Vec<GitHubPage>,
}

pub struct GitHubConnector<T, C = crate::SystemGitHubClock> {
    descriptor: ConnectorDescriptor,
    client: GitHubClient<T, C>,
    page_cache: Mutex<BTreeMap<String, CachedPages>>,
    route_cache: Mutex<Vec<RepositoryRouteContext>>,
}

impl<T> GitHubConnector<T, crate::SystemGitHubClock> {
    pub fn new(transport: T) -> Self {
        Self::with_clock(transport, crate::SystemGitHubClock)
    }
}

impl<T, C> GitHubConnector<T, C> {
    pub fn with_clock(transport: T, clock: C) -> Self {
        Self {
            descriptor: github_descriptor(),
            client: GitHubClient::with_clock(transport, clock),
            page_cache: Mutex::new(BTreeMap::new()),
            route_cache: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl<T, C> ReadConnector for GitHubConnector<T, C>
where
    T: GitHubTransport,
    C: GitHubClock,
{
    fn descriptor(&self) -> &ConnectorDescriptor {
        &self.descriptor
    }

    async fn validate(
        &self,
        request: ValidationRequest,
        secret: Option<&SecretValue>,
    ) -> ConnectorResult<ValidationReport> {
        let mut errors = validate_connection_input(&request.connection);
        let Some(secret) = secret else {
            errors.push(ValidationIssue {
                code: ErrorCode::CredentialUnavailable,
                message: "GitHub credential is unavailable".into(),
            });
            return Ok(invalid_report(errors));
        };
        if !errors.is_empty() {
            return Ok(invalid_report(errors));
        }

        let endpoint = GitHubEndpoint::single("authenticated_user", "/user")
            .map_err(ConnectorFailure::from)?;
        let result = self
            .client
            .fetch_pages_with_budget(
                &endpoint,
                secret,
                None,
                GitHubFetchBudget::new(1, 1).map_err(ConnectorFailure::from)?,
            )
            .await;
        match result {
            Ok(GitHubFetch::Pages(pages)) if pages.pages.len() == 1 => {
                let value: serde_json::Value = pages.pages[0]
                    .deserialize()
                    .map_err(ConnectorFailure::from)?;
                if value.is_object() {
                    Ok(ValidationReport {
                        status: ValidationStatus::Valid,
                        warnings: Vec::new(),
                        errors: Vec::new(),
                    })
                } else {
                    Ok(invalid_report(vec![ValidationIssue {
                        code: ErrorCode::InvalidResponse,
                        message: "GitHub authenticated-user response is invalid".into(),
                    }]))
                }
            }
            Ok(GitHubFetch::NotModified { .. }) => Ok(invalid_report(vec![ValidationIssue {
                code: ErrorCode::InvalidResponse,
                message: "GitHub validation returned not-modified without a cache".into(),
            }])),
            Ok(GitHubFetch::Pages(_)) => Ok(invalid_report(vec![ValidationIssue {
                code: ErrorCode::InvalidResponse,
                message: "GitHub validation returned an unexpected page count".into(),
            }])),
            Err(failure) => Ok(invalid_report(vec![ValidationIssue {
                code: failure.failure.code,
                message: failure.failure.message,
            }])),
        }
    }

    async fn sync(
        &self,
        request: SyncRequest,
        secret: Option<&SecretValue>,
    ) -> ConnectorResult<SyncOutcome> {
        let errors = validate_connection_input(&request.connection);
        if let Some(issue) = errors.into_iter().next() {
            return Err(ConnectorFailure {
                code: issue.code,
                message: issue.message,
                retryable: false,
                retry_after_ms: None,
            });
        }
        let secret = secret.ok_or_else(|| ConnectorFailure {
            code: ErrorCode::CredentialUnavailable,
            message: "GitHub credential is unavailable".into(),
            retryable: false,
            retry_after_ms: None,
        })?;

        let routes = match request.mode {
            SyncMode::Full => None,
            SyncMode::Targeted => Some(self.targeted_routes(&request)?),
            SyncMode::Incremental => {
                return Err(ConnectorFailure {
                    code: ErrorCode::InvalidDomainValue,
                    message: "GitHub incremental sync is not supported".into(),
                    retryable: false,
                    retry_after_ms: None,
                });
            }
        };

        let observed_at = Timestamp::from_unix_millis(
            i64::try_from(self.client.now_epoch_seconds().saturating_mul(1_000))
                .unwrap_or(i64::MAX),
        )
        .expect("system time is non-negative");
        let mut collector = Collector::new(request.scope.clone(), observed_at);
        let repository_output = match routes {
            None => {
                self.collect_repositories(secret, &request, &mut collector)
                    .await?
            }
            Some(routes) => {
                self.collect_targeted_repositories(secret, &request, routes, &mut collector)
                    .await?
            }
        };
        let active_routes = repository_output.routes.clone();
        if request.mode == SyncMode::Full {
            *self.route_cache.lock().expect("route cache mutex") = active_routes.clone();
        }
        collector.merge_repository(repository_output);

        for route in active_routes {
            if collector.remaining_requests == 0 {
                collector.mark_budget_exhausted();
                break;
            }
            self.collect_repository_children(secret, &route, &mut collector)
                .await?;
        }

        collector.finish(&request)
    }
}

impl<T, C> GitHubConnector<T, C>
where
    T: GitHubTransport,
    C: GitHubClock,
{
    fn targeted_routes(
        &self,
        request: &SyncRequest,
    ) -> ConnectorResult<Vec<RepositoryRouteContext>> {
        if request.targeted_resources.is_empty() {
            return Err(invalid_failure(
                "GitHub targeted sync requires repository locators",
            ));
        }
        let cache = self.route_cache.lock().expect("route cache mutex");
        request
            .targeted_resources
            .iter()
            .map(|locator| {
                if locator.kind != ResourceKind::new("github.repository").expect("static kind") {
                    return Err(invalid_failure(
                        "GitHub targeted sync accepts only repository locators",
                    ));
                }
                find_targeted_route(&cache, &locator.external_id).cloned()
            })
            .collect()
    }

    async fn collect_repositories(
        &self,
        secret: &SecretValue,
        request: &SyncRequest,
        collector: &mut Collector,
    ) -> ConnectorResult<RepositoryMapperOutput> {
        let endpoint = GitHubEndpoint::new(
            "repositories",
            "/user/repos",
            &[
                ("visibility", "all"),
                ("affiliation", "owner,collaborator,organization_member"),
                ("sort", "full_name"),
                ("direction", "asc"),
            ],
        )
        .map_err(ConnectorFailure::from)?;
        let fetched = self
            .fetch_cached(&endpoint, secret, 20, collector.remaining_requests)
            .await;
        let (pages, summary, failure) = required_pages(fetched)?;
        collector.add_summary(summary);
        let selected = selected_repository_ids(&request.connection.config)?;
        let repositories = deserialize_array::<RepositoryDto>(&pages)?
            .into_iter()
            .filter(|repository| selected.contains(&repository.id.to_string()))
            .collect::<Vec<_>>();
        map_repositories(
            &request.scope,
            collector.observed_at,
            repositories,
            failure.is_some(),
            failure,
        )
    }

    async fn collect_targeted_repositories(
        &self,
        secret: &SecretValue,
        request: &SyncRequest,
        routes: Vec<RepositoryRouteContext>,
        collector: &mut Collector,
    ) -> ConnectorResult<RepositoryMapperOutput> {
        let mut output = empty_repository_output();
        for route in routes {
            if collector.remaining_requests == 0 {
                collector.mark_budget_exhausted();
                break;
            }
            let endpoint = route.endpoint("repository", "", &[])?;
            let fetched = self
                .fetch_cached(&endpoint, secret, 1, collector.remaining_requests)
                .await;
            let (pages, summary, failure) = required_pages(fetched)?;
            collector.add_summary(summary);
            let mut repositories = deserialize_single::<RepositoryDto>(&pages)?;
            let mapped = map_repositories(
                &request.scope,
                collector.observed_at,
                repositories.drain(..),
                failure.is_some(),
                failure,
            )?;
            output = output.merge(mapped);
        }
        Ok(output)
    }

    async fn collect_repository_children(
        &self,
        secret: &SecretValue,
        route: &RepositoryRouteContext,
        collector: &mut Collector,
    ) -> ConnectorResult<()> {
        let environment = route.endpoint("environments", "environments", &[])?;
        let fetched = self
            .fetch_cached(&environment, secret, 1, collector.remaining_requests)
            .await;
        let (pages, summary, failure) = optional_pages(fetched)?;
        collector.add_summary(summary);
        let environments = deserialize_wrapped(&pages, |dto: EnvironmentListDto| dto.environments)?;
        collector.merge_repository(map_environments(
            route,
            environments,
            failure.is_some(),
            failure,
        )?);

        if collector.remaining_requests == 0 {
            collector.mark_budget_exhausted();
            return Ok(());
        }
        let deployment = route.endpoint("deployments", "deployments", &[])?;
        let fetched = self
            .fetch_cached(&deployment, secret, 2, collector.remaining_requests)
            .await;
        let (pages, summary, failure) = optional_pages(fetched)?;
        collector.add_summary(summary);
        collector.merge_repository(map_deployments(
            route,
            deserialize_array::<DeploymentDto>(&pages)?,
            failure.is_some(),
            failure,
        )?);

        if collector.remaining_requests == 0 {
            collector.mark_budget_exhausted();
            return Ok(());
        }
        let actions_context = GitHubRepositoryContext {
            repository_external_id: route.repository_external_id().clone(),
            scope: route.scope().clone(),
            observed_at: route.observed_at(),
        };
        let workflows_endpoint = route.endpoint("workflows", "actions/workflows", &[])?;
        let fetched = self
            .fetch_cached(&workflows_endpoint, secret, 2, collector.remaining_requests)
            .await;
        let (pages, summary, failure) = optional_pages(fetched)?;
        collector.add_summary(summary);
        let workflows = deserialize_wrapped(&pages, |dto: WorkflowListDto| dto.workflows)?;
        collector.merge_actions(map_workflows(
            &actions_context,
            workflows,
            failure.is_some(),
            failure,
        )?);

        if collector.remaining_requests == 0 {
            collector.mark_budget_exhausted();
            return Ok(());
        }
        let runs_endpoint = route.endpoint("runs", "actions/runs", &[])?;
        let fetched = self
            .fetch_cached(&runs_endpoint, secret, 1, collector.remaining_requests)
            .await;
        let (pages, summary, failure) = optional_pages(fetched)?;
        collector.add_summary(summary);
        let mut runs = deserialize_wrapped(&pages, |dto: WorkflowRunListDto| dto.workflow_runs)?;
        runs.sort_by_key(|run| run.id);
        let runs_bounded = runs.len() > crate::actions::MAX_RUNS_PER_REPOSITORY;
        runs.truncate(crate::actions::MAX_RUNS_PER_REPOSITORY);
        collector.merge_actions(map_runs(
            &actions_context,
            runs.clone(),
            runs_bounded || failure.is_some(),
            failure,
        )?);

        let mut jobs_seen = 0usize;
        for run in runs {
            if collector.remaining_requests == 0
                || jobs_seen >= crate::actions::MAX_JOBS_PER_REPOSITORY
            {
                collector.mark_budget_exhausted();
                break;
            }
            let jobs_endpoint =
                route.endpoint("jobs", &format!("actions/runs/{}/jobs", run.id), &[])?;
            let fetched = self
                .fetch_cached(&jobs_endpoint, secret, 2, collector.remaining_requests)
                .await;
            let (pages, summary, failure) = optional_pages(fetched)?;
            collector.add_summary(summary);
            let mut jobs = deserialize_wrapped(&pages, |dto: JobListDto| dto.jobs)?;
            let remaining = crate::actions::MAX_JOBS_PER_REPOSITORY - jobs_seen;
            let bounded = jobs.len() > remaining || failure.is_some();
            jobs.truncate(remaining);
            jobs_seen += jobs.len();
            collector.merge_actions(map_jobs(&actions_context, jobs, bounded, failure)?);
        }
        Ok(())
    }

    async fn fetch_cached(
        &self,
        endpoint: &GitHubEndpoint,
        secret: &SecretValue,
        max_pages: usize,
        remaining_requests: u64,
    ) -> Result<GitHubPages, GitHubPaginationFailure> {
        if remaining_requests == 0 {
            return Err(GitHubPaginationFailure {
                completed_pages: Vec::new(),
                request_summary: ProviderRequestSummary::default(),
                failure: bounded_failure(),
            });
        }
        let key = endpoint.cache_key().to_owned();
        let cached = self
            .page_cache
            .lock()
            .expect("page cache mutex")
            .get(&key)
            .cloned();
        let result = self
            .client
            .fetch_pages_with_budget(
                endpoint,
                secret,
                cached.as_ref().and_then(|entry| entry.etag.as_deref()),
                GitHubFetchBudget::new(max_pages, remaining_requests.min(MAX_REQUESTS_PER_BATCH))
                    .map_err(|error| GitHubPaginationFailure {
                    completed_pages: Vec::new(),
                    request_summary: ProviderRequestSummary::default(),
                    failure: error.into(),
                })?,
            )
            .await?;
        match result {
            GitHubFetch::Pages(pages) => {
                self.page_cache.lock().expect("page cache mutex").insert(
                    key,
                    CachedPages {
                        etag: pages.etag.clone(),
                        pages: pages.pages.clone(),
                    },
                );
                Ok(pages)
            }
            GitHubFetch::NotModified {
                request_summary, ..
            } => match cached {
                Some(cached) => Ok(GitHubPages {
                    pages: cached.pages,
                    etag: cached.etag,
                    request_summary,
                }),
                None => Err(GitHubPaginationFailure {
                    completed_pages: Vec::new(),
                    request_summary,
                    failure: invalid_failure("GitHub returned not-modified without cached pages"),
                }),
            },
        }
    }
}

fn selected_repository_ids(config: &serde_json::Value) -> ConnectorResult<BTreeSet<String>> {
    let Some(values) = config
        .get("selected_repository_ids")
        .and_then(serde_json::Value::as_array)
    else {
        return Err(invalid_failure(
            "GitHub sync requires selected repositories",
        ));
    };
    let selected = values
        .iter()
        .filter_map(serde_json::Value::as_str)
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    if selected.is_empty() {
        return Err(invalid_failure(
            "GitHub sync requires selected repositories",
        ));
    }
    Ok(selected)
}

struct Collector {
    scope: Scope,
    observed_at: Timestamp,
    resources: Vec<ResourceObservation>,
    relations: Vec<RelationObservation>,
    warnings: Vec<ObservationWarning>,
    summary: ProviderRequestSummary,
    primary_failure: Option<ConnectorFailure>,
    remaining_requests: u64,
}

impl Collector {
    fn new(scope: Scope, observed_at: Timestamp) -> Self {
        Self {
            scope,
            observed_at,
            resources: Vec::new(),
            relations: Vec::new(),
            warnings: Vec::new(),
            summary: ProviderRequestSummary::default(),
            primary_failure: None,
            remaining_requests: MAX_REQUESTS_PER_BATCH,
        }
    }

    fn add_summary(&mut self, summary: ProviderRequestSummary) {
        self.remaining_requests = self
            .remaining_requests
            .saturating_sub(summary.request_count);
        self.summary.request_count = self
            .summary
            .request_count
            .saturating_add(summary.request_count);
        self.summary.elapsed_ms = self.summary.elapsed_ms.saturating_add(summary.elapsed_ms);
        for (class, count) in summary.status_class_counts {
            *self.summary.status_class_counts.entry(class).or_default() += count;
        }
    }

    fn merge_repository(&mut self, output: RepositoryMapperOutput) {
        self.resources.extend(output.resources);
        self.relations.extend(output.relations);
        self.collect_module_gaps(
            output
                .modules
                .into_iter()
                .map(|module| (module.module, module.bounded, module.failure)),
        );
        self.warnings.extend(output.warnings);
    }

    fn merge_actions(&mut self, output: ActionMapperOutput) {
        self.resources.extend(output.resources);
        self.relations.extend(output.relations);
        self.collect_module_gaps(
            output
                .modules
                .into_iter()
                .map(|module| (module.module, module.bounded, module.failure)),
        );
        self.warnings.extend(output.warnings);
    }

    fn collect_module_gaps(
        &mut self,
        modules: impl IntoIterator<Item = (&'static str, bool, Option<ConnectorFailure>)>,
    ) {
        for (module, bounded, failure) in modules {
            if let Some(failure) = failure {
                self.primary_failure.get_or_insert_with(|| failure.clone());
                self.warnings.push(ObservationWarning {
                    code: failure.code,
                    message: format!("GitHub module {module} is partial"),
                });
            } else if bounded {
                self.primary_failure.get_or_insert_with(bounded_failure);
                self.warnings.push(ObservationWarning {
                    code: ErrorCode::PartialPagination,
                    message: format!("GitHub module {module} reached its bounded view"),
                });
            }
        }
    }

    fn mark_budget_exhausted(&mut self) {
        self.primary_failure.get_or_insert_with(bounded_failure);
        self.warnings.push(ObservationWarning {
            code: ErrorCode::PartialPagination,
            message: "GitHub request budget was exhausted".into(),
        });
    }

    fn finish(mut self, request: &SyncRequest) -> ConnectorResult<SyncOutcome> {
        self.resources
            .sort_by_key(|resource| (resource.kind.clone(), resource.external_id.clone()));
        self.relations.sort_by_key(|relation| {
            (
                relation.source.kind.clone(),
                relation.source.external_id.clone(),
                relation.target.kind.clone(),
                relation.target.external_id.clone(),
                relation.kind.clone(),
                relation.evidence_key.clone(),
            )
        });
        reject_duplicate_observations(&self.resources, &self.relations)?;
        let failure = self.primary_failure.unwrap_or_else(bounded_failure);
        let batch = ObservationBatch {
            resources: self.resources,
            relations: self.relations,
            coverage: SyncCoverage::Partial {
                scope: Some(self.scope),
                reason: coverage_reason(failure.code),
            },
            next_cursor: None,
            warnings: self.warnings,
            redaction_report: RedactionReport::default(),
            provider_request_summary: self.summary,
        };
        let outcome = SyncOutcome::Partial { batch, failure };
        outcome
            .validate_for(request)
            .map_err(|_| invalid_failure("GitHub sync outcome violates the connector contract"))?;
        Ok(outcome)
    }
}

fn validate_connection_input(connection: &ConnectionInput) -> Vec<ValidationIssue> {
    let mut errors = Vec::new();
    if connection.connector_type != ConnectorType::new("github").expect("static connector") {
        errors.push(ValidationIssue {
            code: ErrorCode::InvalidDomainValue,
            message: "GitHub connection uses a different connector type".into(),
        });
    }
    if connection.config_schema_version != SchemaVersion::new(1).expect("static schema") {
        errors.push(ValidationIssue {
            code: ErrorCode::SchemaIncompatible,
            message: "GitHub connection config schema is unsupported".into(),
        });
    }
    if connection
        .config
        .get("selected_repository_ids")
        .and_then(serde_json::Value::as_array)
        .is_none_or(|values| values.is_empty())
    {
        errors.push(ValidationIssue {
            code: ErrorCode::InvalidDomainValue,
            message: "GitHub connection config requires selected repositories".into(),
        });
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

fn required_pages(
    result: Result<GitHubPages, GitHubPaginationFailure>,
) -> ConnectorResult<(
    Vec<GitHubPage>,
    ProviderRequestSummary,
    Option<ConnectorFailure>,
)> {
    match result {
        Ok(pages) => Ok((pages.pages, pages.request_summary, None)),
        Err(failure) if failure.completed_pages.is_empty() => Err(failure.failure),
        Err(failure) => {
            fatal_if_auth(&failure.failure)?;
            Ok((
                failure.completed_pages,
                failure.request_summary,
                Some(failure.failure),
            ))
        }
    }
}

fn optional_pages(
    result: Result<GitHubPages, GitHubPaginationFailure>,
) -> ConnectorResult<(
    Vec<GitHubPage>,
    ProviderRequestSummary,
    Option<ConnectorFailure>,
)> {
    match result {
        Ok(pages) => Ok((pages.pages, pages.request_summary, None)),
        Err(failure) => {
            fatal_if_auth(&failure.failure)?;
            Ok((
                failure.completed_pages,
                failure.request_summary,
                Some(failure.failure),
            ))
        }
    }
}

fn fatal_if_auth(failure: &ConnectorFailure) -> ConnectorResult<()> {
    if matches!(
        failure.code,
        ErrorCode::AuthenticationFailed | ErrorCode::CredentialUnavailable
    ) {
        Err(failure.clone())
    } else {
        Ok(())
    }
}

fn deserialize_array<T: DeserializeOwned>(pages: &[GitHubPage]) -> ConnectorResult<Vec<T>> {
    let mut values = Vec::new();
    for page in pages {
        values.extend(
            page.deserialize::<Vec<T>>()
                .map_err(ConnectorFailure::from)?,
        );
    }
    Ok(values)
}

fn deserialize_single<T: DeserializeOwned>(pages: &[GitHubPage]) -> ConnectorResult<Vec<T>> {
    pages
        .iter()
        .map(|page| page.deserialize::<T>().map_err(ConnectorFailure::from))
        .collect()
}

fn deserialize_wrapped<T: DeserializeOwned, V>(
    pages: &[GitHubPage],
    project: impl Fn(T) -> Vec<V>,
) -> ConnectorResult<Vec<V>> {
    let mut values = Vec::new();
    for page in pages {
        values.extend(project(
            page.deserialize::<T>().map_err(ConnectorFailure::from)?,
        ));
    }
    Ok(values)
}

fn empty_repository_output() -> RepositoryMapperOutput {
    RepositoryMapperOutput {
        resources: Vec::new(),
        relations: Vec::new(),
        modules: Vec::new(),
        warnings: Vec::new(),
        routes: Vec::new(),
    }
}

fn reject_duplicate_observations(
    resources: &[ResourceObservation],
    relations: &[RelationObservation],
) -> ConnectorResult<()> {
    let resource_keys = resources
        .iter()
        .map(|resource| (&resource.kind, &resource.external_id))
        .collect::<BTreeSet<_>>();
    if resource_keys.len() != resources.len() {
        return Err(invalid_failure(
            "GitHub collector produced duplicate resources",
        ));
    }
    let relation_keys = relations
        .iter()
        .map(|relation| {
            (
                &relation.source.kind,
                &relation.source.external_id,
                &relation.target.kind,
                &relation.target.external_id,
                &relation.kind,
                &relation.evidence_key,
            )
        })
        .collect::<BTreeSet<_>>();
    if relation_keys.len() != relations.len() {
        return Err(invalid_failure(
            "GitHub collector produced duplicate relations",
        ));
    }
    Ok(())
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

fn bounded_failure() -> ConnectorFailure {
    ConnectorFailure {
        code: ErrorCode::PartialPagination,
        message: "GitHub connector provides a bounded current view".into(),
        retryable: false,
        retry_after_ms: None,
    }
}

fn invalid_failure(message: impl Into<String>) -> ConnectorFailure {
    ConnectorFailure {
        code: ErrorCode::InvalidResponse,
        message: message.into(),
        retryable: false,
        retry_after_ms: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GitHubResponseHeaders, GitHubTransportRequest, GitHubTransportResponse};
    use reqwest::StatusCode;
    use std::sync::{Arc, Mutex};

    type RequestLog = Arc<Mutex<Vec<(String, bool)>>>;

    struct FixedClock(u64);

    impl GitHubClock for FixedClock {
        fn now_epoch_seconds(&self) -> u64 {
            self.0
        }
    }

    struct FakeTransport {
        responses: Mutex<Vec<Result<GitHubTransportResponse, crate::GitHubError>>>,
        requests: RequestLog,
    }

    impl FakeTransport {
        fn new(
            responses: Vec<Result<GitHubTransportResponse, crate::GitHubError>>,
        ) -> (Self, RequestLog) {
            let requests = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    responses: Mutex::new(responses.into_iter().rev().collect()),
                    requests: requests.clone(),
                },
                requests,
            )
        }
    }

    #[async_trait]
    impl GitHubTransport for FakeTransport {
        async fn execute(
            &self,
            request: GitHubTransportRequest,
        ) -> Result<GitHubTransportResponse, crate::GitHubError> {
            assert!(request.authorization_is_sensitive());
            self.requests
                .lock()
                .unwrap()
                .push((request.url().path().to_owned(), request.etag().is_some()));
            self.responses.lock().unwrap().pop().unwrap()
        }
    }

    fn response(
        status: StatusCode,
        etag: Option<&str>,
        body: &'static [u8],
    ) -> Result<GitHubTransportResponse, crate::GitHubError> {
        Ok(GitHubTransportResponse::synthetic(
            status,
            GitHubResponseHeaders {
                etag: etag.map(str::to_owned),
                ..Default::default()
            },
            body,
        ))
    }

    fn full_responses() -> Vec<Result<GitHubTransportResponse, crate::GitHubError>> {
        vec![
            response(
                StatusCode::OK,
                Some("repo-etag"),
                br#"[{"id":10,"name":"fixture-repo","owner":{"login":"fixture-owner"},"visibility":"private","default_branch":"main","archived":false,"disabled":false,"created_at":"2026-08-05T00:00:00Z","updated_at":"2026-08-05T00:01:00Z"}]"#,
            ),
            response(
                StatusCode::OK,
                Some("environment-etag"),
                br#"{"total_count":1,"environments":[{"id":20,"name":"fixture-environment","deployment_branch_policy":{"protected_branches":true,"custom_branch_policies":false}}]}"#,
            ),
            response(
                StatusCode::OK,
                Some("deployment-etag"),
                br#"[{"id":30,"environment":"fixture-environment","task":"deploy","transient_environment":false,"production_environment":true,"created_at":"2026-08-05T00:00:00Z","updated_at":"2026-08-05T00:01:00Z"}]"#,
            ),
            response(
                StatusCode::OK,
                Some("workflow-etag"),
                br#"{"total_count":1,"workflows":[{"id":40,"name":"Fixture workflow","path":".github/workflows/fixture.yml","state":"active","created_at":"2026-08-05T00:00:00Z","updated_at":"2026-08-05T00:01:00Z"}]}"#,
            ),
            response(
                StatusCode::OK,
                Some("run-etag"),
                br#"{"total_count":1,"workflow_runs":[{"id":50,"workflow_id":40,"name":"Fixture workflow","display_title":"Fixture run","run_number":1,"run_attempt":1,"event":"push","status":"completed","conclusion":"success","head_branch":"fixture-branch","created_at":"2026-08-05T00:00:00Z","updated_at":"2026-08-05T00:01:00Z","run_started_at":"2026-08-05T00:00:10Z"}]}"#,
            ),
            response(
                StatusCode::OK,
                Some("job-etag"),
                br#"{"total_count":1,"jobs":[{"id":60,"run_id":50,"name":"Fixture job","status":"completed","conclusion":"success","started_at":"2026-08-05T00:00:20Z","completed_at":"2026-08-05T00:00:50Z"}]}"#,
            ),
        ]
    }

    fn targeted_responses() -> Vec<Result<GitHubTransportResponse, crate::GitHubError>> {
        let mut responses = full_responses();
        responses[0] = response(
            StatusCode::OK,
            Some("targeted-repo-etag"),
            br#"{"id":10,"name":"fixture-repo","owner":{"login":"fixture-owner"},"visibility":"private","default_branch":"main","archived":false,"disabled":false,"created_at":"2026-08-05T00:00:00Z","updated_at":"2026-08-05T00:01:00Z"}"#,
        );
        responses
    }

    fn sync_request() -> SyncRequest {
        SyncRequest {
            sync_run_id: SyncRunId::new("github-connector-fixture-run").unwrap(),
            connection: ConnectionInput {
                connection_id: ConnectionId::new("github-fixture-connection").unwrap(),
                connector_type: ConnectorType::new("github").unwrap(),
                config: serde_json::json!({"selected_repository_ids": ["10"]}),
                config_schema_version: SchemaVersion::new(1).unwrap(),
            },
            mode: SyncMode::Full,
            scope: Scope::new("github-account-scope").unwrap(),
            cursor: None,
            targeted_resources: Vec::new(),
        }
    }

    fn targeted_request() -> SyncRequest {
        let mut request = sync_request();
        request.mode = SyncMode::Targeted;
        request.targeted_resources = vec![ResourceLocator {
            kind: ResourceKind::new("github.repository").unwrap(),
            external_id: ExternalId::new("github-repository:10").unwrap(),
        }];
        request
    }

    #[tokio::test]
    async fn full_vertical_collects_six_resources_and_only_allowlisted_endpoints() {
        let (transport, requests) = FakeTransport::new(full_responses());
        let connector = GitHubConnector::with_clock(transport, FixedClock(1_000));
        let outcome = connector
            .sync(sync_request(), Some(&SecretValue::new("fixture-token")))
            .await
            .unwrap();
        let SyncOutcome::Partial { batch, failure } = outcome else {
            panic!("GitHub bounded view must be partial")
        };
        assert_eq!(batch.resources.len(), 6);
        assert_eq!(batch.relations.len(), 5);
        assert_eq!(batch.provider_request_summary.request_count, 6);
        assert_eq!(failure.code, ErrorCode::PartialPagination);
        let serialized = serde_json::to_string(&batch).unwrap().to_ascii_lowercase();
        for forbidden in ["logs", "artifacts", "secrets", "variables", "statuses"] {
            assert!(!serialized.contains(forbidden));
        }
        let requests = requests.lock().unwrap();
        assert_eq!(
            requests
                .iter()
                .map(|(path, _)| path.as_str())
                .collect::<Vec<_>>(),
            vec![
                "/user/repos",
                "/repos/fixture-owner/fixture-repo/environments",
                "/repos/fixture-owner/fixture-repo/deployments",
                "/repos/fixture-owner/fixture-repo/actions/workflows",
                "/repos/fixture-owner/fixture-repo/actions/runs",
                "/repos/fixture-owner/fixture-repo/actions/runs/50/jobs",
            ]
        );
    }

    #[tokio::test]
    async fn second_full_sync_reuses_all_cached_pages_on_304() {
        let mut responses = full_responses();
        responses.extend((0..6).map(|_| response(StatusCode::NOT_MODIFIED, None, b"")));
        let (transport, requests) = FakeTransport::new(responses);
        let connector = GitHubConnector::with_clock(transport, FixedClock(1_000));
        connector
            .sync(sync_request(), Some(&SecretValue::new("fixture-token")))
            .await
            .unwrap();
        let outcome = connector
            .sync(sync_request(), Some(&SecretValue::new("fixture-token")))
            .await
            .unwrap();
        let SyncOutcome::Partial { batch, .. } = outcome else {
            panic!("GitHub bounded view must be partial")
        };
        assert_eq!(batch.resources.len(), 6);
        assert_eq!(batch.provider_request_summary.status_class_counts["3xx"], 6);
        let requests = requests.lock().unwrap();
        assert!(requests[6..].iter().all(|(_, has_etag)| *has_etag));
    }

    #[tokio::test]
    async fn repository_auth_failure_and_uncached_304_are_fatal() {
        let (transport, _) =
            FakeTransport::new(vec![response(StatusCode::UNAUTHORIZED, None, b"denied")]);
        let auth = GitHubConnector::with_clock(transport, FixedClock(1_000));
        let failure = auth
            .sync(sync_request(), Some(&SecretValue::new("fixture-token")))
            .await
            .unwrap_err();
        assert_eq!(failure.code, ErrorCode::AuthenticationFailed);

        let (transport, _) =
            FakeTransport::new(vec![response(StatusCode::NOT_MODIFIED, None, b"")]);
        let uncached = GitHubConnector::with_clock(transport, FixedClock(1_000));
        let failure = uncached
            .sync(sync_request(), Some(&SecretValue::new("fixture-token")))
            .await
            .unwrap_err();
        assert_eq!(failure.code, ErrorCode::InvalidResponse);
    }

    #[tokio::test]
    async fn targeted_sync_requires_and_reuses_exact_route_cache() {
        let mut responses = full_responses();
        responses.extend(targeted_responses());
        let (transport, requests) = FakeTransport::new(responses);
        let connector = GitHubConnector::with_clock(transport, FixedClock(1_000));
        connector
            .sync(sync_request(), Some(&SecretValue::new("fixture-token")))
            .await
            .unwrap();
        let outcome = connector
            .sync(targeted_request(), Some(&SecretValue::new("fixture-token")))
            .await
            .unwrap();
        let SyncOutcome::Partial { batch, .. } = outcome else {
            panic!("GitHub targeted bounded view must be partial")
        };
        assert_eq!(batch.resources.len(), 6);
        {
            let requests = requests.lock().unwrap();
            assert_eq!(requests[6].0, "/repos/fixture-owner/fixture-repo");
        }

        let (transport, _) = FakeTransport::new(Vec::new());
        let empty = GitHubConnector::with_clock(transport, FixedClock(1_000));
        let failure = empty
            .sync(targeted_request(), Some(&SecretValue::new("fixture-token")))
            .await
            .unwrap_err();
        assert_eq!(failure.code, ErrorCode::InvalidResponse);
    }

    #[tokio::test]
    async fn child_permission_failure_is_partial_and_other_modules_survive() {
        let mut responses = full_responses();
        responses[1] = response(StatusCode::FORBIDDEN, None, b"permission-body-sentinel");
        let (transport, _) = FakeTransport::new(responses);
        let connector = GitHubConnector::with_clock(transport, FixedClock(1_000));
        let outcome = connector
            .sync(sync_request(), Some(&SecretValue::new("fixture-token")))
            .await
            .unwrap();
        let SyncOutcome::Partial { batch, failure } = outcome else {
            panic!("GitHub child permission gap must be partial")
        };
        assert_eq!(failure.code, ErrorCode::PermissionDenied);
        assert_eq!(batch.resources.len(), 5);
        assert_eq!(batch.relations.len(), 4);
        assert!(batch.warnings.iter().any(|warning| {
            warning.code == ErrorCode::PermissionDenied
                && warning.message == "GitHub module github.environments is partial"
        }));
        assert!(!format!("{batch:?}").contains("permission-body-sentinel"));
    }

    #[tokio::test]
    async fn validation_uses_read_only_authenticated_user_endpoint() {
        let (transport, requests) = FakeTransport::new(vec![response(
            StatusCode::OK,
            None,
            br#"{"id":1,"login":"fixture-user","token":"unknown-field-sentinel"}"#,
        )]);
        let connector = GitHubConnector::with_clock(transport, FixedClock(1_000));
        let report = connector
            .validate(
                ValidationRequest {
                    connection: sync_request().connection,
                },
                Some(&SecretValue::new("fixture-token")),
            )
            .await
            .unwrap();
        assert_eq!(report.status, ValidationStatus::Valid);
        assert!(report.errors.is_empty());
        assert_eq!(requests.lock().unwrap()[0].0, "/user");
    }
}
