use crate::{
    GITHUB_API_ORIGIN, GitHubError, GitHubTransport, GitHubTransportRequest,
    GitHubTransportResponse, MAX_RESPONSE_BODY_BYTES, auth::authorization_header,
};
use next_infra_connector_api::{ConnectorFailure, ProviderRequestSummary};
use next_infra_core::{ErrorCode, SecretValue};
use reqwest::{StatusCode, Url, header::HeaderValue};
use serde::de::DeserializeOwned;
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_PAGES_PER_ENDPOINT: usize = 20;
pub const MAX_REQUESTS_PER_BATCH: u64 = 200;
const MAX_RETRY_AFTER_MS: u64 = 60 * 60 * 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GitHubFetchBudget {
    pub max_pages: usize,
    pub max_requests: u64,
}

impl GitHubFetchBudget {
    pub fn new(max_pages: usize, max_requests: u64) -> Result<Self, GitHubError> {
        if max_pages == 0
            || max_pages > MAX_PAGES_PER_ENDPOINT
            || max_requests == 0
            || max_requests > MAX_REQUESTS_PER_BATCH
        {
            return Err(GitHubError::invalid_response(
                "GitHub fetch budget exceeds the transport contract",
            ));
        }
        Ok(Self {
            max_pages,
            max_requests,
        })
    }
}

impl Default for GitHubFetchBudget {
    fn default() -> Self {
        Self {
            max_pages: MAX_PAGES_PER_ENDPOINT,
            max_requests: MAX_REQUESTS_PER_BATCH,
        }
    }
}

pub trait GitHubClock: Send + Sync {
    fn now_epoch_seconds(&self) -> u64;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemGitHubClock;

impl GitHubClock for SystemGitHubClock {
    fn now_epoch_seconds(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_secs())
    }
}

#[derive(Clone)]
pub struct GitHubEndpoint {
    endpoint_class: &'static str,
    first_url: Url,
}

impl GitHubEndpoint {
    pub fn single(endpoint_class: &'static str, path: &str) -> Result<Self, GitHubError> {
        Self::build(endpoint_class, path, &[], false)
    }

    pub fn new(
        endpoint_class: &'static str,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<Self, GitHubError> {
        Self::build(endpoint_class, path, query, true)
    }

    fn build(
        endpoint_class: &'static str,
        path: &str,
        query: &[(&str, &str)],
        add_default_page_size: bool,
    ) -> Result<Self, GitHubError> {
        if endpoint_class.is_empty()
            || !path.starts_with('/')
            || path.starts_with("//")
            || path.contains(['?', '#'])
        {
            return Err(GitHubError::invalid_response(
                "GitHub endpoint class or path is invalid",
            ));
        }
        let origin = Url::parse(GITHUB_API_ORIGIN).expect("static GitHub API origin");
        let mut first_url = origin
            .join(path)
            .map_err(|_| GitHubError::invalid_response("GitHub endpoint path is invalid"))?;
        if first_url.origin() != origin.origin() || first_url.fragment().is_some() {
            return Err(GitHubError::invalid_response(
                "GitHub endpoint must remain on the API origin",
            ));
        }
        {
            let mut pairs = first_url.query_pairs_mut();
            for (key, value) in query {
                if !is_allowed_query_key(key) {
                    return Err(GitHubError::invalid_response(
                        "GitHub endpoint contains an unsupported query key",
                    ));
                }
                pairs.append_pair(key, value);
            }
            if add_default_page_size && !query.iter().any(|(key, _)| *key == "per_page") {
                pairs.append_pair("per_page", "100");
            }
        }
        Ok(Self {
            endpoint_class,
            first_url,
        })
    }

    pub fn endpoint_class(&self) -> &'static str {
        self.endpoint_class
    }

    pub(crate) fn cache_key(&self) -> &str {
        self.first_url.as_str()
    }
}

impl std::fmt::Debug for GitHubEndpoint {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GitHubEndpoint")
            .field("endpoint_class", &self.endpoint_class)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct GitHubPage {
    body: Vec<u8>,
}

impl GitHubPage {
    pub fn deserialize<T: DeserializeOwned>(&self) -> Result<T, GitHubError> {
        serde_json::from_slice(&self.body).map_err(|_| {
            GitHubError::invalid_response("GitHub response body is not valid expected JSON")
        })
    }

    pub fn body_len(&self) -> usize {
        self.body.len()
    }
}

impl std::fmt::Debug for GitHubPage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GitHubPage")
            .field("body_bytes", &self.body.len())
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitHubPages {
    pub pages: Vec<GitHubPage>,
    pub etag: Option<String>,
    pub request_summary: ProviderRequestSummary,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitHubFetch {
    NotModified {
        etag: Option<String>,
        request_summary: ProviderRequestSummary,
    },
    Pages(GitHubPages),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitHubPaginationFailure {
    pub completed_pages: Vec<GitHubPage>,
    pub request_summary: ProviderRequestSummary,
    pub failure: ConnectorFailure,
}

impl GitHubPaginationFailure {
    pub fn is_partial(&self) -> bool {
        !self.completed_pages.is_empty()
    }
}

pub struct GitHubClient<T, C = SystemGitHubClock> {
    transport: T,
    clock: C,
}

impl<T> GitHubClient<T, SystemGitHubClock> {
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            clock: SystemGitHubClock,
        }
    }
}

impl<T, C> GitHubClient<T, C> {
    pub fn with_clock(transport: T, clock: C) -> Self {
        Self { transport, clock }
    }
}

impl<T, C> GitHubClient<T, C>
where
    T: GitHubTransport,
    C: GitHubClock,
{
    pub(crate) fn now_epoch_seconds(&self) -> u64 {
        self.clock.now_epoch_seconds()
    }

    pub async fn fetch_pages(
        &self,
        endpoint: &GitHubEndpoint,
        secret: &SecretValue,
        etag: Option<&str>,
    ) -> Result<GitHubFetch, GitHubPaginationFailure> {
        self.fetch_pages_with_budget(endpoint, secret, etag, GitHubFetchBudget::default())
            .await
    }

    pub async fn fetch_pages_with_budget(
        &self,
        endpoint: &GitHubEndpoint,
        secret: &SecretValue,
        etag: Option<&str>,
        budget: GitHubFetchBudget,
    ) -> Result<GitHubFetch, GitHubPaginationFailure> {
        if GitHubFetchBudget::new(budget.max_pages, budget.max_requests).is_err() {
            return Err(pagination_failure(
                Vec::new(),
                summary(),
                GitHubError::invalid_response("GitHub fetch budget exceeds the transport contract"),
            ));
        }
        let authorization = authorization_header(secret)
            .map_err(|error| pagination_failure(Vec::new(), summary(), error))?;
        let request_etag = etag.map(HeaderValue::from_str).transpose().map_err(|_| {
            pagination_failure(
                Vec::new(),
                summary(),
                GitHubError::invalid_response("GitHub ETag contains invalid header bytes"),
            )
        })?;

        let mut url = endpoint.first_url.clone();
        let mut completed_pages = Vec::new();
        let mut seen_urls = BTreeSet::new();
        let mut request_summary = summary();
        let mut first_etag = None;

        loop {
            if completed_pages.len() >= budget.max_pages
                || request_summary.request_count >= budget.max_requests
            {
                return Err(pagination_failure(
                    completed_pages,
                    request_summary,
                    GitHubError::new(
                        ErrorCode::PartialPagination,
                        "GitHub pagination exceeded its request budget",
                        false,
                        None,
                    ),
                ));
            }
            if !seen_urls.insert(url.as_str().to_owned()) {
                return Err(pagination_failure(
                    completed_pages,
                    request_summary,
                    GitHubError::invalid_response("GitHub pagination contains a cycle"),
                ));
            }

            let response = self
                .transport
                .execute(GitHubTransportRequest::new(
                    url,
                    authorization.clone(),
                    if completed_pages.is_empty() {
                        request_etag.clone()
                    } else {
                        None
                    },
                    endpoint.endpoint_class,
                ))
                .await;
            request_summary.request_count += 1;
            let response = match response {
                Ok(response) => response,
                Err(error) => {
                    return Err(pagination_failure(completed_pages, request_summary, error));
                }
            };
            request_summary.elapsed_ms = request_summary
                .elapsed_ms
                .saturating_add(response.elapsed_ms);
            count_status(&mut request_summary, response.status);

            if response.status == StatusCode::NOT_MODIFIED {
                if !completed_pages.is_empty() {
                    return Err(pagination_failure(
                        completed_pages,
                        request_summary,
                        GitHubError::invalid_response(
                            "GitHub returned not-modified after pagination began",
                        ),
                    ));
                }
                return Ok(GitHubFetch::NotModified {
                    etag: response.headers.etag,
                    request_summary,
                });
            }
            if !response.status.is_success() {
                let error = classify_status(&response, self.clock.now_epoch_seconds());
                return Err(pagination_failure(completed_pages, request_summary, error));
            }
            if response.body.len() > MAX_RESPONSE_BODY_BYTES {
                return Err(pagination_failure(
                    completed_pages,
                    request_summary,
                    GitHubError::invalid_response("GitHub response exceeds the body budget"),
                ));
            }

            if first_etag.is_none() {
                first_etag = response.headers.etag.clone();
            }
            let page = GitHubPage {
                body: response.body,
            };
            let next = match response.headers.link.as_deref() {
                Some(link) => match next_url(link) {
                    Ok(next) => next,
                    Err(error) => {
                        completed_pages.push(page);
                        return Err(pagination_failure(completed_pages, request_summary, error));
                    }
                },
                None => None,
            };
            completed_pages.push(page);
            match next {
                Some(next) => url = next,
                None => {
                    return Ok(GitHubFetch::Pages(GitHubPages {
                        pages: completed_pages,
                        etag: first_etag,
                        request_summary,
                    }));
                }
            }
        }
    }
}

fn is_allowed_query_key(key: &str) -> bool {
    matches!(
        key,
        "per_page"
            | "page"
            | "visibility"
            | "affiliation"
            | "type"
            | "sort"
            | "direction"
            | "status"
            | "environment"
            | "ref"
            | "sha"
            | "branch"
            | "event"
            | "created"
            | "exclude_pull_requests"
            | "check_suite_id"
            | "filter"
    )
}

fn next_url(link: &str) -> Result<Option<Url>, GitHubError> {
    let mut next = None;
    for entry in link.split(',') {
        let mut parts = entry.trim().split(';');
        let Some(target) = parts.next() else {
            continue;
        };
        let is_next = parts.any(|part| part.trim() == "rel=\"next\"");
        if !is_next {
            continue;
        }
        let raw = target
            .trim()
            .strip_prefix('<')
            .and_then(|value| value.strip_suffix('>'))
            .ok_or_else(|| GitHubError::invalid_response("GitHub Link header is malformed"))?;
        let url = Url::parse(raw)
            .map_err(|_| GitHubError::invalid_response("GitHub next link is not a valid URL"))?;
        validate_api_url(&url)?;
        if next.replace(url).is_some() {
            return Err(GitHubError::invalid_response(
                "GitHub Link header contains multiple next targets",
            ));
        }
    }
    Ok(next)
}

fn validate_api_url(url: &Url) -> Result<(), GitHubError> {
    let origin = Url::parse(GITHUB_API_ORIGIN).expect("static GitHub API origin");
    if url.origin() != origin.origin()
        || url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.fragment().is_some()
    {
        return Err(GitHubError::invalid_response(
            "GitHub pagination left the approved API origin",
        ));
    }
    if url
        .query_pairs()
        .any(|(key, _)| !is_allowed_query_key(&key))
    {
        return Err(GitHubError::invalid_response(
            "GitHub pagination contains an unsupported query key",
        ));
    }
    Ok(())
}

fn classify_status(response: &GitHubTransportResponse, now_epoch_seconds: u64) -> GitHubError {
    let status = response.status;
    if status == StatusCode::UNAUTHORIZED {
        return GitHubError::authentication("GitHub rejected the configured credential");
    }
    let rate_limited = status == StatusCode::TOO_MANY_REQUESTS
        || (status == StatusCode::FORBIDDEN
            && (response.headers.retry_after.is_some()
                || response.headers.rate_limit_remaining.as_deref() == Some("0")));
    if rate_limited {
        return GitHubError::new(
            ErrorCode::RateLimited,
            "GitHub rate limit prevented the request",
            true,
            retry_after_ms(&response.headers, now_epoch_seconds),
        );
    }
    match status {
        StatusCode::FORBIDDEN => GitHubError::new(
            ErrorCode::PermissionDenied,
            "GitHub token lacks permission for the requested module",
            false,
            None,
        ),
        StatusCode::NOT_FOUND => GitHubError::new(
            ErrorCode::NotFound,
            "GitHub endpoint or requested resource was not found",
            false,
            None,
        ),
        status if status.is_server_error() => GitHubError::new(
            ErrorCode::ProviderUnavailable,
            "GitHub REST API is temporarily unavailable",
            true,
            None,
        ),
        _ => GitHubError::invalid_response("GitHub returned an unexpected HTTP status"),
    }
}

fn retry_after_ms(headers: &crate::GitHubResponseHeaders, now_epoch_seconds: u64) -> Option<u64> {
    headers
        .retry_after
        .as_deref()
        .and_then(|value| value.parse::<u64>().ok())
        .map(|seconds| seconds.saturating_mul(1000).min(MAX_RETRY_AFTER_MS))
        .or_else(|| {
            headers
                .rate_limit_reset
                .as_deref()
                .and_then(|value| value.parse::<u64>().ok())
                .map(|reset| {
                    reset
                        .saturating_sub(now_epoch_seconds)
                        .saturating_mul(1000)
                        .min(MAX_RETRY_AFTER_MS)
                })
        })
}

fn summary() -> ProviderRequestSummary {
    ProviderRequestSummary {
        request_count: 0,
        elapsed_ms: 0,
        status_class_counts: BTreeMap::new(),
    }
}

fn count_status(summary: &mut ProviderRequestSummary, status: StatusCode) {
    let class = format!("{}xx", status.as_u16() / 100);
    *summary.status_class_counts.entry(class).or_default() += 1;
}

fn pagination_failure(
    completed_pages: Vec<GitHubPage>,
    request_summary: ProviderRequestSummary,
    error: GitHubError,
) -> GitHubPaginationFailure {
    GitHubPaginationFailure {
        completed_pages,
        request_summary,
        failure: error.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GitHubResponseHeaders, GitHubTransportResponse};
    use async_trait::async_trait;
    use std::sync::Mutex;

    struct FakeClock(u64);

    impl GitHubClock for FakeClock {
        fn now_epoch_seconds(&self) -> u64 {
            self.0
        }
    }

    struct FakeTransport {
        responses: Mutex<Vec<Result<GitHubTransportResponse, GitHubError>>>,
        requests: Mutex<Vec<GitHubTransportRequest>>,
    }

    impl FakeTransport {
        fn new(responses: Vec<Result<GitHubTransportResponse, GitHubError>>) -> Self {
            Self {
                responses: Mutex::new(responses.into_iter().rev().collect()),
                requests: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl GitHubTransport for FakeTransport {
        async fn execute(
            &self,
            request: GitHubTransportRequest,
        ) -> Result<GitHubTransportResponse, GitHubError> {
            self.requests.lock().unwrap().push(request);
            self.responses.lock().unwrap().pop().unwrap()
        }
    }

    fn response(
        status: StatusCode,
        headers: GitHubResponseHeaders,
        body: &'static [u8],
    ) -> Result<GitHubTransportResponse, GitHubError> {
        Ok(GitHubTransportResponse::synthetic(status, headers, body))
    }

    fn endpoint() -> GitHubEndpoint {
        GitHubEndpoint::new("repositories", "/user/repos", &[]).unwrap()
    }

    #[tokio::test]
    async fn pagination_is_bounded_same_origin_and_redacted() {
        let next = format!("<{GITHUB_API_ORIGIN}/user/repos?page=2&per_page=100>; rel=\"next\"");
        let transport = FakeTransport::new(vec![
            response(
                StatusCode::OK,
                GitHubResponseHeaders {
                    etag: Some("etag-v1".into()),
                    link: Some(next),
                    ..Default::default()
                },
                br#"[{"id":1}]"#,
            ),
            response(
                StatusCode::OK,
                GitHubResponseHeaders::default(),
                br#"[{"id":2}]"#,
            ),
        ]);
        let client = GitHubClient::new(transport);
        let result = client
            .fetch_pages(&endpoint(), &SecretValue::new("token-sentinel"), None)
            .await
            .unwrap();
        let GitHubFetch::Pages(pages) = result else {
            panic!("expected pages")
        };
        assert_eq!(pages.pages.len(), 2);
        assert_eq!(pages.etag.as_deref(), Some("etag-v1"));
        assert_eq!(pages.request_summary.request_count, 2);
        assert!(!format!("{pages:?}").contains("token-sentinel"));
    }

    #[tokio::test]
    async fn etag_not_modified_is_typed_and_not_an_empty_page() {
        let transport = FakeTransport::new(vec![response(
            StatusCode::NOT_MODIFIED,
            GitHubResponseHeaders {
                etag: Some("etag-v1".into()),
                ..Default::default()
            },
            b"",
        )]);
        let client = GitHubClient::new(transport);
        let result = client
            .fetch_pages(
                &endpoint(),
                &SecretValue::new("token-sentinel"),
                Some("etag-v1"),
            )
            .await
            .unwrap();
        assert!(matches!(result, GitHubFetch::NotModified { .. }));
    }

    #[tokio::test]
    async fn status_classification_distinguishes_permission_and_rate_limit() {
        let permission = FakeTransport::new(vec![response(
            StatusCode::FORBIDDEN,
            GitHubResponseHeaders::default(),
            b"body-sentinel",
        )]);
        let failure = GitHubClient::with_clock(permission, FakeClock(100))
            .fetch_pages(&endpoint(), &SecretValue::new("token"), None)
            .await
            .unwrap_err();
        assert_eq!(failure.failure.code, ErrorCode::PermissionDenied);
        assert!(!format!("{failure:?}").contains("body-sentinel"));

        let limited = FakeTransport::new(vec![response(
            StatusCode::FORBIDDEN,
            GitHubResponseHeaders {
                rate_limit_remaining: Some("0".into()),
                rate_limit_reset: Some("130".into()),
                ..Default::default()
            },
            b"rate-body-sentinel",
        )]);
        let failure = GitHubClient::with_clock(limited, FakeClock(100))
            .fetch_pages(&endpoint(), &SecretValue::new("token"), None)
            .await
            .unwrap_err();
        assert_eq!(failure.failure.code, ErrorCode::RateLimited);
        assert_eq!(failure.failure.retry_after_ms, Some(30_000));
        assert!(!format!("{failure:?}").contains("rate-body-sentinel"));
    }

    #[tokio::test]
    async fn cross_origin_next_link_fails_after_preserving_completed_page() {
        let transport = FakeTransport::new(vec![response(
            StatusCode::OK,
            GitHubResponseHeaders {
                link: Some("<https://example.test/repos?page=2>; rel=\"next\"".into()),
                ..Default::default()
            },
            br#"[{"id":1}]"#,
        )]);
        let failure = GitHubClient::new(transport)
            .fetch_pages(&endpoint(), &SecretValue::new("token"), None)
            .await
            .unwrap_err();
        assert_eq!(failure.failure.code, ErrorCode::InvalidResponse);
        assert!(failure.is_partial());
        assert_eq!(failure.completed_pages.len(), 1);
    }

    #[tokio::test]
    async fn second_page_failure_is_partial() {
        let next = format!("<{GITHUB_API_ORIGIN}/user/repos?page=2&per_page=100>; rel=\"next\"");
        let transport = FakeTransport::new(vec![
            response(
                StatusCode::OK,
                GitHubResponseHeaders {
                    link: Some(next),
                    ..Default::default()
                },
                br#"[{"id":1}]"#,
            ),
            Err(GitHubError::network("GitHub request failed")),
        ]);
        let failure = GitHubClient::new(transport)
            .fetch_pages(&endpoint(), &SecretValue::new("token"), None)
            .await
            .unwrap_err();
        assert!(failure.is_partial());
        assert_eq!(failure.completed_pages.len(), 1);
    }

    #[test]
    fn endpoint_rejects_unsupported_query_and_hides_url_in_debug() {
        assert!(GitHubEndpoint::new("repositories", "/user/repos", &[("token", "x")]).is_err());
        assert!(GitHubEndpoint::new("repositories", "/user/repos?token=x", &[]).is_err());
        let endpoint =
            GitHubEndpoint::new("repositories", "/repos/private-owner/private-repo", &[]).unwrap();
        assert!(!format!("{endpoint:?}").contains("private-owner"));
    }
}
