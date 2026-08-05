use crate::{GITHUB_API_VERSION, GitHubError};
use async_trait::async_trait;
use reqwest::{
    Client, Method, StatusCode, Url,
    header::{
        ACCEPT, AUTHORIZATION, ETAG, HeaderMap, HeaderValue, IF_NONE_MATCH, LINK, RETRY_AFTER,
        USER_AGENT,
    },
    redirect::Policy,
};
use std::{fmt, time::Duration};

pub const MAX_RESPONSE_BODY_BYTES: usize = 4 * 1024 * 1024;
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

pub struct GitHubTransportRequest {
    url: Url,
    authorization: HeaderValue,
    etag: Option<HeaderValue>,
    endpoint_class: &'static str,
}

impl GitHubTransportRequest {
    pub(crate) fn new(
        url: Url,
        authorization: HeaderValue,
        etag: Option<HeaderValue>,
        endpoint_class: &'static str,
    ) -> Self {
        Self {
            url,
            authorization,
            etag,
            endpoint_class,
        }
    }

    pub fn endpoint_class(&self) -> &'static str {
        self.endpoint_class
    }

    pub fn url(&self) -> &Url {
        &self.url
    }

    pub fn authorization_is_sensitive(&self) -> bool {
        self.authorization.is_sensitive()
    }

    pub fn etag(&self) -> Option<&HeaderValue> {
        self.etag.as_ref()
    }
}

impl fmt::Debug for GitHubTransportRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GitHubTransportRequest")
            .field("endpoint_class", &self.endpoint_class)
            .field("has_etag", &self.etag.is_some())
            .field("authorization", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, Default, PartialEq, Eq)]
pub struct GitHubResponseHeaders {
    pub etag: Option<String>,
    pub link: Option<String>,
    pub retry_after: Option<String>,
    pub rate_limit_limit: Option<String>,
    pub rate_limit_remaining: Option<String>,
    pub rate_limit_reset: Option<String>,
    pub rate_limit_resource: Option<String>,
}

impl fmt::Debug for GitHubResponseHeaders {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GitHubResponseHeaders")
            .field("has_etag", &self.etag.is_some())
            .field("has_link", &self.link.is_some())
            .field("has_retry_after", &self.retry_after.is_some())
            .field("has_rate_limit", &self.rate_limit_limit.is_some())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct GitHubTransportResponse {
    pub status: StatusCode,
    pub headers: GitHubResponseHeaders,
    pub body: Vec<u8>,
    pub elapsed_ms: u64,
}

impl GitHubTransportResponse {
    pub fn synthetic(
        status: StatusCode,
        headers: GitHubResponseHeaders,
        body: impl Into<Vec<u8>>,
    ) -> Self {
        Self {
            status,
            headers,
            body: body.into(),
            elapsed_ms: 0,
        }
    }
}

impl fmt::Debug for GitHubTransportResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GitHubTransportResponse")
            .field("status", &self.status)
            .field("headers", &self.headers)
            .field("body_bytes", &self.body.len())
            .field("elapsed_ms", &self.elapsed_ms)
            .finish()
    }
}

#[async_trait]
pub trait GitHubTransport: Send + Sync {
    async fn execute(
        &self,
        request: GitHubTransportRequest,
    ) -> Result<GitHubTransportResponse, GitHubError>;
}

pub struct ReqwestGitHubTransport {
    client: Client,
}

impl ReqwestGitHubTransport {
    pub fn new() -> Result<Self, GitHubError> {
        let client = Client::builder()
            .redirect(Policy::none())
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|_| GitHubError::network("failed to initialize GitHub HTTP client"))?;
        Ok(Self { client })
    }
}

impl Default for ReqwestGitHubTransport {
    fn default() -> Self {
        Self::new().expect("static GitHub HTTP client configuration is valid")
    }
}

#[async_trait]
impl GitHubTransport for ReqwestGitHubTransport {
    async fn execute(
        &self,
        request: GitHubTransportRequest,
    ) -> Result<GitHubTransportResponse, GitHubError> {
        let started = std::time::Instant::now();
        let request = build_request(&self.client, request)?;
        let mut response = self
            .client
            .execute(request)
            .await
            .map_err(classify_reqwest_error)?;
        if response.content_length().is_some_and(|size| {
            size > u64::try_from(MAX_RESPONSE_BODY_BYTES).expect("body budget fits u64")
        }) {
            return Err(GitHubError::invalid_response(
                "GitHub response exceeds the body budget",
            ));
        }

        let status = response.status();
        let headers = allowlisted_headers(response.headers());
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(classify_reqwest_error)? {
            if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BODY_BYTES {
                return Err(GitHubError::invalid_response(
                    "GitHub response exceeds the body budget",
                ));
            }
            body.extend_from_slice(&chunk);
        }
        Ok(GitHubTransportResponse {
            status,
            headers,
            body,
            elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        })
    }
}

fn build_request(
    client: &Client,
    request: GitHubTransportRequest,
) -> Result<reqwest::Request, GitHubError> {
    let mut builder = client
        .request(Method::GET, request.url)
        .header(ACCEPT, "application/vnd.github+json")
        .header(USER_AGENT, "next-infra/0.1")
        .header("X-GitHub-Api-Version", GITHUB_API_VERSION)
        .header(AUTHORIZATION, request.authorization);
    if let Some(etag) = request.etag {
        builder = builder.header(IF_NONE_MATCH, etag);
    }
    builder
        .build()
        .map_err(|_| GitHubError::invalid_response("failed to build GitHub HTTP request"))
}

fn classify_reqwest_error(error: reqwest::Error) -> GitHubError {
    if error.is_timeout() || error.is_connect() {
        GitHubError::network("GitHub request timed out or could not connect")
    } else {
        GitHubError::new(
            next_infra_core::ErrorCode::ProviderUnavailable,
            "GitHub request failed",
            true,
            None,
        )
    }
}

fn allowlisted_headers(headers: &HeaderMap) -> GitHubResponseHeaders {
    GitHubResponseHeaders {
        etag: header_text(headers, ETAG),
        link: header_text(headers, LINK),
        retry_after: header_text(headers, RETRY_AFTER),
        rate_limit_limit: header_text_by_name(headers, "x-ratelimit-limit"),
        rate_limit_remaining: header_text_by_name(headers, "x-ratelimit-remaining"),
        rate_limit_reset: header_text_by_name(headers, "x-ratelimit-reset"),
        rate_limit_resource: header_text_by_name(headers, "x-ratelimit-resource"),
    }
}

fn header_text(headers: &HeaderMap, name: reqwest::header::HeaderName) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

fn header_text_by_name(headers: &HeaderMap, name: &'static str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepared_request_has_exact_fixed_headers_and_sensitive_authorization() {
        let client = Client::builder().build().unwrap();
        let mut authorization = HeaderValue::from_static("Bearer token-sentinel");
        authorization.set_sensitive(true);
        let request = build_request(
            &client,
            GitHubTransportRequest::new(
                Url::parse("https://api.github.com/user/repos?per_page=100").unwrap(),
                authorization,
                Some(HeaderValue::from_static("etag-v1")),
                "repositories",
            ),
        )
        .unwrap();

        assert_eq!(request.method(), Method::GET);
        assert_eq!(
            request.headers().get(ACCEPT).unwrap(),
            "application/vnd.github+json"
        );
        assert_eq!(request.headers().get(USER_AGENT).unwrap(), "next-infra/0.1");
        assert_eq!(
            request.headers().get("X-GitHub-Api-Version").unwrap(),
            GITHUB_API_VERSION
        );
        assert_eq!(request.headers().get(IF_NONE_MATCH).unwrap(), "etag-v1");
        assert!(request.headers().get(AUTHORIZATION).unwrap().is_sensitive());
        assert!(!format!("{request:?}").contains("token-sentinel"));
    }
}
