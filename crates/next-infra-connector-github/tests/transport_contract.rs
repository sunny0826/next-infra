use async_trait::async_trait;
use next_infra_connector_github::{
    GITHUB_API_ORIGIN, GitHubClient, GitHubClock, GitHubEndpoint, GitHubError, GitHubFetch,
    GitHubResponseHeaders, GitHubTransport, GitHubTransportRequest, GitHubTransportResponse,
    MAX_PAGES_PER_ENDPOINT, MAX_RESPONSE_BODY_BYTES,
};
use next_infra_core::{ErrorCode, SecretValue};
use reqwest::StatusCode;
use std::sync::Mutex;

struct FixedClock(u64);

impl GitHubClock for FixedClock {
    fn now_epoch_seconds(&self) -> u64 {
        self.0
    }
}

struct FakeTransport {
    responses: Mutex<Vec<Result<GitHubTransportResponse, GitHubError>>>,
}

impl FakeTransport {
    fn new(responses: Vec<Result<GitHubTransportResponse, GitHubError>>) -> Self {
        Self {
            responses: Mutex::new(responses.into_iter().rev().collect()),
        }
    }
}

#[async_trait]
impl GitHubTransport for FakeTransport {
    async fn execute(
        &self,
        request: GitHubTransportRequest,
    ) -> Result<GitHubTransportResponse, GitHubError> {
        assert_eq!(request.endpoint_class(), "repositories");
        assert!(request.authorization_is_sensitive());
        self.responses.lock().unwrap().pop().unwrap()
    }
}

fn endpoint() -> GitHubEndpoint {
    GitHubEndpoint::new("repositories", "/user/repos", &[]).unwrap()
}

fn response(
    status: StatusCode,
    headers: GitHubResponseHeaders,
    body: impl Into<Vec<u8>>,
) -> Result<GitHubTransportResponse, GitHubError> {
    Ok(GitHubTransportResponse::synthetic(status, headers, body))
}

async fn failure_for(
    status: StatusCode,
    headers: GitHubResponseHeaders,
) -> next_infra_connector_github::GitHubPaginationFailure {
    GitHubClient::with_clock(
        FakeTransport::new(vec![response(status, headers, b"response-body-sentinel")]),
        FixedClock(100),
    )
    .fetch_pages(&endpoint(), &SecretValue::new("fixture-token"), None)
    .await
    .unwrap_err()
}

#[tokio::test]
async fn status_matrix_is_structured_and_body_redacted() {
    let cases = [
        (
            StatusCode::UNAUTHORIZED,
            ErrorCode::AuthenticationFailed,
            false,
        ),
        (StatusCode::FORBIDDEN, ErrorCode::PermissionDenied, false),
        (StatusCode::NOT_FOUND, ErrorCode::NotFound, false),
        (
            StatusCode::BAD_GATEWAY,
            ErrorCode::ProviderUnavailable,
            true,
        ),
    ];
    for (status, code, retryable) in cases {
        let failure = failure_for(status, GitHubResponseHeaders::default()).await;
        assert_eq!(failure.failure.code, code);
        assert_eq!(failure.failure.retryable, retryable);
        assert!(!format!("{failure:?}").contains("response-body-sentinel"));
    }
}

#[tokio::test]
async fn retry_after_has_precedence_and_is_clamped_to_one_hour() {
    let failure = failure_for(
        StatusCode::TOO_MANY_REQUESTS,
        GitHubResponseHeaders {
            retry_after: Some("7200".into()),
            rate_limit_remaining: Some("0".into()),
            rate_limit_reset: Some("101".into()),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(failure.failure.code, ErrorCode::RateLimited);
    assert_eq!(failure.failure.retry_after_ms, Some(3_600_000));
}

#[tokio::test]
async fn synthetic_oversized_body_is_rejected_before_deserialization() {
    let transport = FakeTransport::new(vec![response(
        StatusCode::OK,
        GitHubResponseHeaders::default(),
        vec![b'x'; MAX_RESPONSE_BODY_BYTES + 1],
    )]);
    let failure = GitHubClient::new(transport)
        .fetch_pages(&endpoint(), &SecretValue::new("fixture-token"), None)
        .await
        .unwrap_err();
    assert_eq!(failure.failure.code, ErrorCode::InvalidResponse);
    assert!(!failure.is_partial());
}

#[tokio::test]
async fn duplicate_next_targets_fail_with_the_current_page_preserved() {
    let link = format!(
        "<{GITHUB_API_ORIGIN}/user/repos?page=2>; rel=\"next\", <{GITHUB_API_ORIGIN}/user/repos?page=3>; rel=\"next\""
    );
    let transport = FakeTransport::new(vec![response(
        StatusCode::OK,
        GitHubResponseHeaders {
            link: Some(link),
            ..Default::default()
        },
        br#"[{"id":1}]"#,
    )]);
    let failure = GitHubClient::new(transport)
        .fetch_pages(&endpoint(), &SecretValue::new("fixture-token"), None)
        .await
        .unwrap_err();
    assert_eq!(failure.failure.code, ErrorCode::InvalidResponse);
    assert_eq!(failure.completed_pages.len(), 1);
}

#[tokio::test]
async fn page_budget_stops_before_request_twenty_one() {
    let mut responses = Vec::new();
    for page in 1..=MAX_PAGES_PER_ENDPOINT {
        let next_page = page + 1;
        responses.push(response(
            StatusCode::OK,
            GitHubResponseHeaders {
                link: Some(format!(
                    "<{GITHUB_API_ORIGIN}/user/repos?page={next_page}&per_page=100>; rel=\"next\""
                )),
                ..Default::default()
            },
            br#"[{"id":1}]"#,
        ));
    }
    let failure = GitHubClient::new(FakeTransport::new(responses))
        .fetch_pages(&endpoint(), &SecretValue::new("fixture-token"), None)
        .await
        .unwrap_err();
    assert_eq!(failure.failure.code, ErrorCode::PartialPagination);
    assert_eq!(failure.completed_pages.len(), MAX_PAGES_PER_ENDPOINT);
    assert_eq!(
        failure.request_summary.request_count,
        u64::try_from(MAX_PAGES_PER_ENDPOINT).unwrap()
    );
}

#[tokio::test]
async fn typed_json_failure_never_exposes_body() {
    let transport = FakeTransport::new(vec![response(
        StatusCode::OK,
        GitHubResponseHeaders::default(),
        b"invalid-json-body-sentinel",
    )]);
    let result = GitHubClient::new(transport)
        .fetch_pages(&endpoint(), &SecretValue::new("fixture-token"), None)
        .await
        .unwrap();
    let GitHubFetch::Pages(pages) = result else {
        panic!("expected pages")
    };
    let error = pages.pages[0]
        .deserialize::<serde_json::Value>()
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidResponse);
    assert!(!format!("{error:?}").contains("invalid-json-body-sentinel"));
}
