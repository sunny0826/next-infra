use crate::{CloudflareAuthError, CloudflareEndpoint, CloudflareRequest};
use async_trait::async_trait;
use next_infra_core::{ErrorCode, SecretValue};
use reqwest::{Client, StatusCode, redirect::Policy};
use serde::Deserialize;
use std::fmt;

pub const MAX_PAGES_PER_ENDPOINT: usize = 100;
pub const MAX_RESPONSE_BODY_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloudflareClientError {
    pub code: ErrorCode,
    pub retry_after_ms: Option<u64>,
}
impl fmt::Display for CloudflareClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Cloudflare request could not be completed")
    }
}
impl std::error::Error for CloudflareClientError {}

#[derive(Clone, PartialEq, Eq)]
pub struct CloudflareResponse {
    pub status: StatusCode,
    pub retry_after_seconds: Option<u64>,
    pub body: Vec<u8>,
}
impl fmt::Debug for CloudflareResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CloudflareResponse")
            .field("status", &self.status)
            .field("body_bytes", &self.body.len())
            .finish()
    }
}

#[async_trait]
pub trait CloudflareTransport: Send + Sync {
    async fn execute(
        &self,
        request: CloudflareRequest,
    ) -> Result<CloudflareResponse, CloudflareClientError>;
}

pub struct ReqwestCloudflareTransport {
    client: Client,
}

impl ReqwestCloudflareTransport {
    pub fn new() -> Result<Self, CloudflareClientError> {
        Client::builder()
            .redirect(Policy::none())
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map(|client| Self { client })
            .map_err(|_| error(ErrorCode::NetworkUnreachable, None))
    }
}

impl Default for ReqwestCloudflareTransport {
    fn default() -> Self {
        Self::new().expect("static Cloudflare HTTP client configuration is valid")
    }
}

#[async_trait]
impl CloudflareTransport for ReqwestCloudflareTransport {
    async fn execute(
        &self,
        request: CloudflareRequest,
    ) -> Result<CloudflareResponse, CloudflareClientError> {
        let started = std::time::Instant::now();
        let response = self
            .client
            .get(request.url)
            .header(reqwest::header::AUTHORIZATION, request.authorization)
            .send()
            .await
            .map_err(|_| error(ErrorCode::NetworkUnreachable, None))?;
        if response
            .content_length()
            .is_some_and(|size| size > MAX_RESPONSE_BODY_BYTES as u64)
        {
            return Err(error(ErrorCode::InvalidResponse, None));
        }
        let status = response.status();
        let retry_after_seconds = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|header| header.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .map(|seconds| seconds.min(3_600));
        let body = response
            .bytes()
            .await
            .map_err(|_| error(ErrorCode::NetworkUnreachable, None))?
            .to_vec();
        if body.len() > MAX_RESPONSE_BODY_BYTES {
            return Err(error(ErrorCode::InvalidResponse, None));
        }
        let _elapsed_ms = started.elapsed().as_millis();
        Ok(CloudflareResponse {
            status,
            retry_after_seconds,
            body,
        })
    }
}

pub struct CloudflareClient<T> {
    transport: T,
}
impl<T> CloudflareClient<T> {
    pub fn new(transport: T) -> Self {
        Self { transport }
    }
}

impl<T: CloudflareTransport> CloudflareClient<T> {
    pub async fn fetch_pages(
        &self,
        path: &str,
        secret: &SecretValue,
    ) -> Result<Vec<Vec<u8>>, CloudflareClientError> {
        let endpoint = CloudflareEndpoint::new();
        let mut page = 1_u64;
        let mut pages = Vec::new();
        loop {
            if pages.len() == MAX_PAGES_PER_ENDPOINT {
                return Err(error(ErrorCode::PartialPagination, None));
            }
            let mut url = endpoint.resource(path).map_err(auth_error)?;
            url.query_pairs_mut()
                .append_pair("page", &page.to_string())
                .append_pair("per_page", "100");
            let mut request = CloudflareRequest::new(path, secret).map_err(auth_error)?;
            request.url = url;
            let response = self.transport.execute(request).await?;
            if response.body.len() > MAX_RESPONSE_BODY_BYTES {
                return Err(error(ErrorCode::InvalidResponse, None));
            }
            if response.status == StatusCode::TOO_MANY_REQUESTS {
                return Err(error(
                    ErrorCode::RateLimited,
                    response
                        .retry_after_seconds
                        .map(|seconds| seconds.saturating_mul(1000)),
                ));
            }
            if response.status == StatusCode::UNAUTHORIZED {
                return Err(error(ErrorCode::AuthenticationFailed, None));
            }
            if response.status == StatusCode::FORBIDDEN {
                return Err(error(ErrorCode::PermissionDenied, None));
            }
            if !response.status.is_success() {
                return Err(error(ErrorCode::ProviderUnavailable, None));
            }
            let total_pages = page_count(&response.body)?;
            pages.push(response.body);
            if page >= total_pages {
                return Ok(pages);
            }
            page = page.saturating_add(1);
        }
    }
}

#[derive(Deserialize)]
struct PageEnvelope {
    result_info: Option<PageInfo>,
}
#[derive(Deserialize)]
struct PageInfo {
    total_pages: Option<u64>,
}
fn page_count(body: &[u8]) -> Result<u64, CloudflareClientError> {
    let envelope: PageEnvelope =
        serde_json::from_slice(body).map_err(|_| error(ErrorCode::InvalidResponse, None))?;
    Ok(envelope
        .result_info
        .and_then(|info| info.total_pages)
        .unwrap_or(1)
        .clamp(1, MAX_PAGES_PER_ENDPOINT as u64))
}
fn error(code: ErrorCode, retry_after_ms: Option<u64>) -> CloudflareClientError {
    CloudflareClientError {
        code,
        retry_after_ms,
    }
}
fn auth_error(_: CloudflareAuthError) -> CloudflareClientError {
    error(ErrorCode::AuthenticationFailed, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    struct Fake {
        responses: Mutex<Vec<Result<CloudflareResponse, CloudflareClientError>>>,
    }
    #[async_trait]
    impl CloudflareTransport for Fake {
        async fn execute(
            &self,
            request: CloudflareRequest,
        ) -> Result<CloudflareResponse, CloudflareClientError> {
            assert!(request.authorization.is_sensitive());
            self.responses.lock().unwrap().pop().unwrap()
        }
    }
    fn ok(page_count: u64) -> Result<CloudflareResponse, CloudflareClientError> {
        Ok(CloudflareResponse {
            status: StatusCode::OK,
            retry_after_seconds: None,
            body: format!(r#"{{"result":[],"result_info":{{"total_pages":{page_count}}}}}"#)
                .into_bytes(),
        })
    }
    #[tokio::test]
    async fn pagination_is_bounded_and_429_is_typed() {
        let client = CloudflareClient::new(Fake {
            responses: Mutex::new(vec![ok(2), ok(2)]),
        });
        assert_eq!(
            client
                .fetch_pages("/client/v4/zones", &SecretValue::new("token"))
                .await
                .unwrap()
                .len(),
            2
        );
        let client = CloudflareClient::new(Fake {
            responses: Mutex::new(vec![Ok(CloudflareResponse {
                status: StatusCode::TOO_MANY_REQUESTS,
                retry_after_seconds: Some(30),
                body: Vec::new(),
            })]),
        });
        let failure = client
            .fetch_pages("/client/v4/zones", &SecretValue::new("token"))
            .await
            .unwrap_err();
        assert_eq!(failure.code, ErrorCode::RateLimited);
        assert_eq!(failure.retry_after_ms, Some(30_000));
    }
}
