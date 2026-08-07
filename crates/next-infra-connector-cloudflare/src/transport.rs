use crate::{CloudflareAuthError, authorization_header};
use next_infra_core::SecretValue;
use reqwest::{Url, header::HeaderValue};

pub const CLOUDFLARE_API_ORIGIN: &str = "https://api.cloudflare.com";

#[derive(Clone)]
pub struct CloudflareEndpoint {
    base_url: Url,
}

impl CloudflareEndpoint {
    pub fn new() -> Self {
        Self {
            base_url: Url::parse(CLOUDFLARE_API_ORIGIN).expect("static Cloudflare API origin"),
        }
    }

    pub fn resource(&self, path: &str) -> Result<Url, CloudflareAuthError> {
        if !path.starts_with('/') || path.contains(['?', '#']) {
            return Err(CloudflareAuthError);
        }
        self.base_url.join(path).map_err(|_| CloudflareAuthError)
    }
}

impl Default for CloudflareEndpoint {
    fn default() -> Self {
        Self::new()
    }
}

pub struct CloudflareRequest {
    pub url: Url,
    pub authorization: HeaderValue,
}

impl CloudflareRequest {
    pub fn new(path: &str, secret: &SecretValue) -> Result<Self, CloudflareAuthError> {
        let endpoint = CloudflareEndpoint::new();
        Ok(Self {
            url: endpoint.resource(path)?,
            authorization: authorization_header(secret)?,
        })
    }
}

impl std::fmt::Debug for CloudflareRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CloudflareRequest")
            .field("path", &self.url.path())
            .field("authorization", &"[REDACTED]")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_is_fixed_to_cloudflare_api_origin_and_redacts_token() {
        let request =
            CloudflareRequest::new("/client/v4/zones", &SecretValue::new("token-sentinel"))
                .unwrap();
        assert_eq!(
            request.url.origin().ascii_serialization(),
            CLOUDFLARE_API_ORIGIN
        );
        assert!(!format!("{request:?}").contains("token-sentinel"));
        assert!(
            CloudflareEndpoint::new()
                .resource("/client/v4/zones?api_token=secret")
                .is_err()
        );
    }
}
