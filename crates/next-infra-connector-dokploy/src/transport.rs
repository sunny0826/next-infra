use crate::{DokployAuthError, authorization_header};
use next_infra_core::SecretValue;
use reqwest::{Url, header::HeaderValue};

#[derive(Clone)]
pub struct DokployEndpoint {
    base_url: Url,
}

impl DokployEndpoint {
    pub fn new(base_url: &str) -> Result<Self, DokployAuthError> {
        let url = Url::parse(base_url).map_err(|_| DokployAuthError)?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(DokployAuthError);
        }
        Ok(Self { base_url: url })
    }

    pub fn resource(&self, path: &str) -> Result<Url, DokployAuthError> {
        if !path.starts_with('/') || path.contains('#') {
            return Err(DokployAuthError);
        }
        self.base_url.join(path).map_err(|_| DokployAuthError)
    }
}

pub struct DokployRequest {
    pub url: Url,
    pub authorization: HeaderValue,
}

impl DokployRequest {
    pub fn new(
        endpoint: &DokployEndpoint,
        path: &str,
        secret: &SecretValue,
    ) -> Result<Self, DokployAuthError> {
        Ok(Self {
            url: endpoint.resource(path)?,
            authorization: authorization_header(secret)?,
        })
    }
}

impl std::fmt::Debug for DokployRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DokployRequest")
            .field("path", &self.url.path())
            .field("authorization", &"[REDACTED]")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_rejects_credentials_and_fragment_in_base_url() {
        assert!(DokployEndpoint::new("dokploy://host").is_err());
        assert!(DokployEndpoint::new("https://user:pass@example.test").is_err());
        assert!(DokployEndpoint::new("https://example.test?token=secret").is_err());
        let endpoint = DokployEndpoint::new("https://dokploy.example.test").unwrap();
        let request = DokployRequest::new(
            &endpoint,
            "/api/project",
            &SecretValue::new("token-sentinel"),
        )
        .unwrap();
        assert!(!format!("{request:?}").contains("token-sentinel"));
    }

    #[test]
    fn endpoint_allows_query_string_in_resource_path() {
        let endpoint = DokployEndpoint::new("https://dokploy.example.test").unwrap();
        let request = DokployRequest::new(
            &endpoint,
            "/api/deployment.all?applicationId=app-123",
            &SecretValue::new("token"),
        )
        .unwrap();
        assert_eq!(
            request.url.as_str(),
            "https://dokploy.example.test/api/deployment.all?applicationId=app-123"
        );
    }

    #[test]
    fn endpoint_rejects_fragment_in_resource_path() {
        let endpoint = DokployEndpoint::new("https://dokploy.example.test").unwrap();
        assert!(
            DokployRequest::new(
                &endpoint,
                "/api/project#fragment",
                &SecretValue::new("token"),
            )
            .is_err()
        );
    }
}
