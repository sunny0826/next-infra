use crate::{DokployAuthError, DokployEndpoint, DokployRequest};
use async_trait::async_trait;
use next_infra_core::{ErrorCode, SecretValue};
use reqwest::{Client, StatusCode, redirect::Policy};
use std::fmt;

pub const MAX_PAGES_PER_ENDPOINT: usize = 100;
pub const MAX_RESPONSE_BODY_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DokployClientError {
    pub code: ErrorCode,
    pub retry_after_ms: Option<u64>,
}
impl fmt::Display for DokployClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Dokploy request could not be completed")
    }
}
impl std::error::Error for DokployClientError {}

#[derive(Clone, PartialEq, Eq)]
pub struct DokployResponse {
    pub status: StatusCode,
    pub retry_after_seconds: Option<u64>,
    pub next_cursor: Option<String>,
    pub body: Vec<u8>,
}
impl fmt::Debug for DokployResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DokployResponse")
            .field("status", &self.status)
            .field("has_next_cursor", &self.next_cursor.is_some())
            .field("body_bytes", &self.body.len())
            .finish()
    }
}

#[async_trait]
pub trait DokployTransport: Send + Sync {
    async fn execute(&self, request: DokployRequest)
    -> Result<DokployResponse, DokployClientError>;
}

pub struct ReqwestDokployTransport {
    client: Client,
}
impl ReqwestDokployTransport {
    pub fn new() -> Result<Self, DokployClientError> {
        Client::builder()
            .redirect(Policy::none())
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map(|client| Self { client })
            .map_err(|_| error(ErrorCode::NetworkUnreachable, None))
    }
}
impl Default for ReqwestDokployTransport {
    fn default() -> Self {
        Self::new().expect("static Dokploy HTTP client configuration is valid")
    }
}
#[async_trait]
impl DokployTransport for ReqwestDokployTransport {
    async fn execute(
        &self,
        request: DokployRequest,
    ) -> Result<DokployResponse, DokployClientError> {
        let response = self
            .client
            .get(request.url)
            .header("accept", "application/json")
            .header("x-api-key", request.authorization)
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
            .and_then(|value| value.parse().ok())
            .map(|seconds: u64| seconds.min(3_600));
        let next_cursor = response
            .headers()
            .get("x-next-cursor")
            .and_then(|header| header.to_str().ok())
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        let body = response
            .bytes()
            .await
            .map_err(|_| error(ErrorCode::NetworkUnreachable, None))?
            .to_vec();
        if body.len() > MAX_RESPONSE_BODY_BYTES {
            return Err(error(ErrorCode::InvalidResponse, None));
        }
        Ok(DokployResponse {
            status,
            retry_after_seconds,
            next_cursor,
            body,
        })
    }
}

pub struct DokployClient<T> {
    transport: T,
    endpoint: DokployEndpoint,
}
impl<T> DokployClient<T> {
    pub fn new(base_url: &str, transport: T) -> Result<Self, DokployClientError> {
        Ok(Self {
            transport,
            endpoint: DokployEndpoint::new(base_url).map_err(auth_error)?,
        })
    }
}
impl<T: DokployTransport> DokployClient<T> {
    pub async fn fetch_pages(
        &self,
        path: &str,
        secret: &SecretValue,
    ) -> Result<Vec<Vec<u8>>, DokployClientError> {
        let mut cursor: Option<String> = None;
        let mut pages = Vec::new();
        loop {
            if pages.len() == MAX_PAGES_PER_ENDPOINT {
                return Err(error(ErrorCode::PartialPagination, None));
            }
            let mut request =
                DokployRequest::new(&self.endpoint, path, secret).map_err(auth_error)?;
            if let Some(cursor) = &cursor {
                request.url.query_pairs_mut().append_pair("cursor", cursor);
            }
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
            pages.push(response.body);
            cursor = response.next_cursor;
            if cursor.is_none() {
                return Ok(pages);
            }
        }
    }
}
fn error(code: ErrorCode, retry_after_ms: Option<u64>) -> DokployClientError {
    DokployClientError {
        code,
        retry_after_ms,
    }
}
fn auth_error(_: DokployAuthError) -> DokployClientError {
    error(ErrorCode::AuthenticationFailed, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    struct Fake {
        responses: Mutex<Vec<Result<DokployResponse, DokployClientError>>>,
    }
    #[async_trait]
    impl DokployTransport for Fake {
        async fn execute(
            &self,
            request: DokployRequest,
        ) -> Result<DokployResponse, DokployClientError> {
            assert!(request.authorization.is_sensitive());
            self.responses.lock().unwrap().pop().unwrap()
        }
    }
    fn page(cursor: Option<&str>) -> Result<DokployResponse, DokployClientError> {
        Ok(DokployResponse {
            status: StatusCode::OK,
            retry_after_seconds: None,
            next_cursor: cursor.map(str::to_owned),
            body: b"[]".to_vec(),
        })
    }
    #[tokio::test]
    async fn cursor_pagination_is_bounded_and_rate_limit_is_typed() {
        let client = DokployClient::new(
            "https://dokploy.example.test",
            Fake {
                responses: Mutex::new(vec![page(None), page(Some("next"))]),
            },
        )
        .unwrap();
        assert_eq!(
            client
                .fetch_pages("/api/project.all", &SecretValue::new("token"))
                .await
                .unwrap()
                .len(),
            2
        );
        let client = DokployClient::new(
            "https://dokploy.example.test",
            Fake {
                responses: Mutex::new(vec![Ok(DokployResponse {
                    status: StatusCode::TOO_MANY_REQUESTS,
                    retry_after_seconds: Some(10),
                    next_cursor: None,
                    body: Vec::new(),
                })]),
            },
        )
        .unwrap();
        let failure = client
            .fetch_pages("/api/project.all", &SecretValue::new("token"))
            .await
            .unwrap_err();
        assert_eq!(failure.code, ErrorCode::RateLimited);
        assert_eq!(failure.retry_after_ms, Some(10_000));
    }
}
