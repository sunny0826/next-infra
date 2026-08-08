//! HARNESS-01: Shared REST live smoke harness for 6 providers.
//!
//! Env-driven CLI that, for each of 6 REST providers, constructs a real reqwest
//! transport implementing that connector's transport trait, runs connector.validate +
//! connector.sync (Full, bounded), validates the outcome with the existing conformance
//! helpers, and prints a SAFE summary.
//!
//! Never prints credentials, raw provider payloads, or hostnames.

use async_trait::async_trait;
use next_infra_connector_aliyun::{AliyunConnector, AliyunTransport, SignedRequest};
use next_infra_connector_api::{
    ConnectionInput, ConnectorFailure, ReadConnector, SyncOutcome, SyncRequest, ValidationReport,
    ValidationRequest, ValidationStatus,
};
use next_infra_connector_cloudflare::{
    CloudflareClientError, CloudflareConnector, CloudflareRequest, CloudflareResponse,
    CloudflareTransport,
};
use next_infra_connector_contract_tests::{check_batch, check_outcome};
use next_infra_connector_dokploy::{
    DokployClientError, DokployConnector, DokployRequest, DokployResponse, DokployTransport,
};
use next_infra_connector_supabase_managed::{
    ManagementRequest, ManagementTransport, SupabaseManagedConnector,
};
use next_infra_connector_supabase_self_hosted::{SelfHostedTransport, SupabaseSelfHostedConnector};
use next_infra_connector_tencent::{
    SignedRequest as TencentSignedRequest, TencentConnector, TencentTransport,
};
use next_infra_core::{
    ConnectionId, ConnectorType, ErrorCode, SchemaVersion, Scope, SecretValue, SyncMode, SyncRunId,
};
use reqwest::Client;
use serde_json::json;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Provider enumeration
// ---------------------------------------------------------------------------

/// Describe a JSON value's shape (keys and types only, never values).
fn json_structure(value: &serde_json::Value) -> String {
    structure_at(value, 0)
}

fn structure_at(value: &serde_json::Value, depth: usize) -> String {
    match value {
        serde_json::Value::Object(map) if depth < 4 => {
            let keys = map
                .iter()
                .map(|(key, value)| format!("{key}:{}", structure_at(value, depth + 1)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("object{{{keys}}}")
        }
        serde_json::Value::Object(_) => "object".into(),
        serde_json::Value::Array(items) => {
            let shape = items
                .first()
                .map(|item| structure_at(item, depth + 1))
                .unwrap_or_else(|| "empty".into());
            format!("array[{shape}]")
        }
        other => json_type(other).into(),
    }
}

fn json_type(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    Dokploy,
    Cloudflare,
    SupabaseManaged,
    SupabaseSelfHosted,
    Aliyun,
    Tencent,
}

impl Provider {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "dokploy" => Some(Self::Dokploy),
            "cloudflare" => Some(Self::Cloudflare),
            "supabase_managed" => Some(Self::SupabaseManaged),
            "supabase_self_hosted" => Some(Self::SupabaseSelfHosted),
            "aliyun" => Some(Self::Aliyun),
            "tencent" => Some(Self::Tencent),
            _ => None,
        }
    }

    pub fn all() -> &'static [Provider] {
        &[
            Self::Dokploy,
            Self::Cloudflare,
            Self::SupabaseManaged,
            Self::SupabaseSelfHosted,
            Self::Aliyun,
            Self::Tencent,
        ]
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Dokploy => "dokploy",
            Self::Cloudflare => "cloudflare",
            Self::SupabaseManaged => "supabase_managed",
            Self::SupabaseSelfHosted => "supabase_self_hosted",
            Self::Aliyun => "aliyun",
            Self::Tencent => "tencent",
        }
    }

    pub fn required_env_vars(&self) -> &'static [&'static str] {
        match self {
            Self::Dokploy => &["NEXT_INFRA_DOKPLOY_URL", "NEXT_INFRA_DOKPLOY_TOKEN"],
            Self::Cloudflare => &["NEXT_INFRA_CLOUDFLARE_TOKEN"],
            Self::SupabaseManaged => &["NEXT_INFRA_SUPABASE_MANAGED_TOKEN"],
            Self::SupabaseSelfHosted => &[
                "NEXT_INFRA_SUPABASE_SELF_HOSTED_URL",
                "NEXT_INFRA_SUPABASE_SELF_HOSTED_SERVICE_KEY",
            ],
            Self::Aliyun => &[
                "NEXT_INFRA_ALIYUN_ACCESS_KEY_ID",
                "NEXT_INFRA_ALIYUN_ACCESS_KEY_SECRET",
                "NEXT_INFRA_ALIYUN_REGION (optional, default cn-hangzhou)",
            ],
            Self::Tencent => &[
                "NEXT_INFRA_TENCENT_SECRET_ID",
                "NEXT_INFRA_TENCENT_SECRET_KEY",
                "NEXT_INFRA_TENCENT_REGION (optional, default ap-guangzhou)",
            ],
        }
    }

    pub fn connector_type(&self) -> ConnectorType {
        match self {
            Self::Dokploy => ConnectorType::new("dokploy").unwrap(),
            Self::Cloudflare => ConnectorType::new("cloudflare").unwrap(),
            Self::SupabaseManaged => ConnectorType::new("supabase-managed").unwrap(),
            Self::SupabaseSelfHosted => ConnectorType::new("supabase-self-hosted").unwrap(),
            Self::Aliyun => ConnectorType::new("aliyun").unwrap(),
            Self::Tencent => ConnectorType::new("tencent").unwrap(),
        }
    }
}

// ---------------------------------------------------------------------------
// Live reqwest transports
// ---------------------------------------------------------------------------

// --- Dokploy ---

pub struct LiveDokployTransport {
    client: Client,
}

impl LiveDokployTransport {
    pub fn new() -> Self {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(15))
            .build()
            .expect("Dokploy HTTP client config is valid");
        Self { client }
    }
}

impl Default for LiveDokployTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for LiveDokployTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveDokployTransport").finish()
    }
}

#[async_trait]
impl DokployTransport for LiveDokployTransport {
    async fn execute(
        &self,
        request: DokployRequest,
    ) -> Result<DokployResponse, DokployClientError> {
        if std::env::var("NEXT_INFRA_DEBUG").is_ok() {
            eprintln!(
                "> GET {} (x-api-key: <{} bytes>)",
                request.url,
                request.authorization.len()
            );
        }
        let response = self
            .client
            .get(request.url)
            .header("accept", "application/json")
            .header("x-api-key", request.authorization)
            .send()
            .await
            .map_err(|_| DokployClientError {
                code: ErrorCode::NetworkUnreachable,
                retry_after_ms: None,
            })?;

        let status = response.status();
        if std::env::var("NEXT_INFRA_DEBUG").is_ok() {
            eprintln!("< HTTP {status}");
        }
        let retry_after_seconds = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|h| h.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .map(|s| s.min(3600));
        let next_cursor = response
            .headers()
            .get("x-next-cursor")
            .and_then(|h| h.to_str().ok())
            .filter(|v| !v.is_empty())
            .map(str::to_owned);
        let body = response
            .bytes()
            .await
            .map_err(|_| DokployClientError {
                code: ErrorCode::NetworkUnreachable,
                retry_after_ms: None,
            })?
            .to_vec();
        if std::env::var("NEXT_INFRA_DEBUG").is_ok()
            && let Ok(value) = serde_json::from_slice::<serde_json::Value>(&body)
        {
            eprintln!("< body structure: {}", json_structure(&value));
            if let (Some(code), Some(message)) = (
                value.get("code").and_then(serde_json::Value::as_str),
                value.get("message").and_then(serde_json::Value::as_str),
            ) {
                eprintln!("< error envelope: code={code} message={message}");
            }
        }

        Ok(DokployResponse {
            status,
            retry_after_seconds,
            next_cursor,
            body,
        })
    }
}

// --- Cloudflare ---

pub struct LiveCloudflareTransport {
    client: Client,
}

impl LiveCloudflareTransport {
    pub fn new() -> Self {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(15))
            .build()
            .expect("Cloudflare HTTP client config is valid");
        Self { client }
    }
}

impl Default for LiveCloudflareTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for LiveCloudflareTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveCloudflareTransport").finish()
    }
}

#[async_trait]
impl CloudflareTransport for LiveCloudflareTransport {
    async fn execute(
        &self,
        request: CloudflareRequest,
    ) -> Result<CloudflareResponse, CloudflareClientError> {
        if std::env::var("NEXT_INFRA_DEBUG").is_ok() {
            eprintln!(
                "> GET {} (Authorization: <{} bytes>)",
                request.url,
                request.authorization.len()
            );
        }
        let response = self
            .client
            .get(request.url)
            .header(reqwest::header::AUTHORIZATION, request.authorization)
            .send()
            .await
            .map_err(|_| CloudflareClientError {
                code: ErrorCode::NetworkUnreachable,
                retry_after_ms: None,
            })?;

        let status = response.status();
        if std::env::var("NEXT_INFRA_DEBUG").is_ok() {
            eprintln!("< HTTP {status}");
        }
        let retry_after_seconds = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|h| h.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .map(|s| s.min(3600));
        let body = response
            .bytes()
            .await
            .map_err(|_| CloudflareClientError {
                code: ErrorCode::NetworkUnreachable,
                retry_after_ms: None,
            })?
            .to_vec();

        Ok(CloudflareResponse {
            status,
            retry_after_seconds,
            body,
        })
    }
}

// --- Supabase Managed ---

pub struct LiveManagementTransport {
    client: Client,
}

impl LiveManagementTransport {
    pub fn new() -> Self {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(15))
            .build()
            .expect("Supabase managed HTTP client config is valid");
        Self { client }
    }
}

impl Default for LiveManagementTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for LiveManagementTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveManagementTransport").finish()
    }
}

#[async_trait]
impl ManagementTransport for LiveManagementTransport {
    async fn get(&self, request: ManagementRequest) -> Result<Vec<u8>, ConnectorFailure> {
        if std::env::var("NEXT_INFRA_DEBUG").is_ok() {
            eprintln!(
                "> GET {} (Authorization: <{} bytes>)",
                request.url,
                request.authorization.len()
            );
        }
        let response = self
            .client
            .get(request.url)
            .header(reqwest::header::AUTHORIZATION, request.authorization)
            .send()
            .await
            .map_err(|_| ConnectorFailure {
                code: ErrorCode::NetworkUnreachable,
                message: "Supabase Management API network error".into(),
                retryable: true,
                retry_after_ms: None,
            })?;

        let status = response.status();
        if std::env::var("NEXT_INFRA_DEBUG").is_ok() {
            eprintln!("< HTTP {status}");
        }
        if status.as_u16() == 429 {
            let retry_after_ms = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|h| h.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .map(|s| s.saturating_mul(1000));
            return Err(ConnectorFailure {
                code: ErrorCode::RateLimited,
                message: "Supabase rate limited".into(),
                retryable: true,
                retry_after_ms,
            });
        }
        if status.as_u16() == 401 {
            return Err(ConnectorFailure {
                code: ErrorCode::AuthenticationFailed,
                message: "Supabase authentication failed".into(),
                retryable: false,
                retry_after_ms: None,
            });
        }
        if !status.is_success() {
            return Err(ConnectorFailure {
                code: ErrorCode::ProviderUnavailable,
                message: format!("Supabase API returned {}", status.as_u16()),
                retryable: false,
                retry_after_ms: None,
            });
        }

        let body = response
            .bytes()
            .await
            .map_err(|_| ConnectorFailure {
                code: ErrorCode::NetworkUnreachable,
                message: "Supabase Management API network error".into(),
                retryable: true,
                retry_after_ms: None,
            })?
            .to_vec();
        Ok(body)
    }
}

// --- Supabase Self-Hosted ---

pub struct LiveSelfHostedTransport {
    client: Client,
    base_url: url::Url,
    service_key: reqwest::header::HeaderValue,
}

impl LiveSelfHostedTransport {
    pub fn new(base_url: &str, service_key: &str) -> Result<Self, String> {
        let base = url::Url::parse(base_url).map_err(|_| "invalid Supabase self-hosted URL")?;
        let mut header = reqwest::header::HeaderValue::from_str(service_key)
            .map_err(|_| "invalid service key")?;
        header.set_sensitive(true);
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(15))
            .build()
            .expect("Supabase self-hosted HTTP client config is valid");
        Ok(Self {
            client,
            base_url: base,
            service_key: header,
        })
    }

    fn openapi_url(&self) -> Result<url::Url, String> {
        self.base_url
            .join("/rest/v1/")
            .map_err(|_| "invalid path".to_string())
    }
}

impl std::fmt::Debug for LiveSelfHostedTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveSelfHostedTransport")
            .field("base_url", &self.base_url)
            .finish()
    }
}

#[async_trait]
impl SelfHostedTransport for LiveSelfHostedTransport {
    async fn read_openapi(&self) -> Result<Vec<u8>, ConnectorFailure> {
        let url = self.openapi_url().map_err(|e| ConnectorFailure {
            code: ErrorCode::InvalidDomainValue,
            message: e,
            retryable: false,
            retry_after_ms: None,
        })?;

        if std::env::var("NEXT_INFRA_DEBUG").is_ok() {
            eprintln!("> GET {url} (apikey: <{} bytes>)", self.service_key.len());
        }
        let response = self
            .client
            .get(url)
            .header("apikey", self.service_key.clone())
            .header("Content-Type", "application/json")
            .send()
            .await
            .map_err(|_| ConnectorFailure {
                code: ErrorCode::NetworkUnreachable,
                message: "Supabase self-hosted network error".into(),
                retryable: true,
                retry_after_ms: None,
            })?;

        let status = response.status();
        if std::env::var("NEXT_INFRA_DEBUG").is_ok() {
            eprintln!("< HTTP {status}");
        }
        if status.as_u16() == 401 {
            return Err(ConnectorFailure {
                code: ErrorCode::AuthenticationFailed,
                message: "Supabase self-hosted authentication failed".into(),
                retryable: false,
                retry_after_ms: None,
            });
        }
        if !status.is_success() {
            return Err(ConnectorFailure {
                code: ErrorCode::ProviderUnavailable,
                message: format!("Supabase self-hosted returned {}", status.as_u16()),
                retryable: false,
                retry_after_ms: None,
            });
        }

        let body = response
            .bytes()
            .await
            .map_err(|_| ConnectorFailure {
                code: ErrorCode::NetworkUnreachable,
                message: "Supabase self-hosted network error".into(),
                retryable: true,
                retry_after_ms: None,
            })?
            .to_vec();
        Ok(body)
    }
}

// --- Aliyun ---

pub struct LiveAliyunTransport {
    client: Client,
}

impl LiveAliyunTransport {
    pub fn new() -> Self {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Aliyun HTTP client config is valid");
        Self { client }
    }
}

impl Default for LiveAliyunTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for LiveAliyunTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveAliyunTransport").finish()
    }
}

#[async_trait]
impl AliyunTransport for LiveAliyunTransport {
    async fn list(
        &self,
        request: SignedRequest,
        _module: &'static str,
    ) -> Result<Vec<u8>, ConnectorFailure> {
        if std::env::var("NEXT_INFRA_DEBUG").is_ok() {
            eprintln!("> GET {}", request.url);
        }
        let response = self
            .client
            .get(request.url)
            .send()
            .await
            .map_err(|_| ConnectorFailure {
                code: ErrorCode::NetworkUnreachable,
                message: "Aliyun network error".into(),
                retryable: true,
                retry_after_ms: None,
            })?;

        let status = response.status();
        if std::env::var("NEXT_INFRA_DEBUG").is_ok() {
            eprintln!("< HTTP {status}");
        }
        if status.as_u16() == 429 {
            let retry_after_ms = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|h| h.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .map(|s| s.saturating_mul(1000));
            return Err(ConnectorFailure {
                code: ErrorCode::RateLimited,
                message: "Aliyun rate limited".into(),
                retryable: true,
                retry_after_ms,
            });
        }
        if status.as_u16() == 403 {
            return Err(ConnectorFailure {
                code: ErrorCode::AuthenticationFailed,
                message: "Aliyun authentication failed".into(),
                retryable: false,
                retry_after_ms: None,
            });
        }
        if !status.is_success() {
            if std::env::var("NEXT_INFRA_DEBUG").is_ok()
                && let Ok(bytes) = response.bytes().await
                && let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes)
            {
                eprintln!(
                    "< aliyun error code={} message={}",
                    value
                        .get("Code")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("?"),
                    value
                        .get("Message")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("?")
                );
            }
            return Err(ConnectorFailure {
                code: ErrorCode::ProviderUnavailable,
                message: format!("Aliyun API returned {}", status.as_u16()),
                retryable: false,
                retry_after_ms: None,
            });
        }

        let body = response
            .bytes()
            .await
            .map_err(|_| ConnectorFailure {
                code: ErrorCode::NetworkUnreachable,
                message: "Aliyun network error".into(),
                retryable: true,
                retry_after_ms: None,
            })?
            .to_vec();
        if std::env::var("NEXT_INFRA_DEBUG").is_ok()
            && let Ok(value) = serde_json::from_slice::<serde_json::Value>(&body)
        {
            eprintln!("< aliyun body: {}", json_structure(&value));
        }
        Ok(body)
    }
}

// --- Tencent ---

pub struct LiveTencentTransport {
    client: Client,
}

impl LiveTencentTransport {
    pub fn new() -> Self {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Tencent HTTP client config is valid");
        Self { client }
    }
}

impl Default for LiveTencentTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for LiveTencentTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveTencentTransport").finish()
    }
}

#[async_trait]
impl TencentTransport for LiveTencentTransport {
    async fn list(
        &self,
        request: TencentSignedRequest,
        _module: &'static str,
    ) -> Result<Vec<u8>, ConnectorFailure> {
        let response = self
            .client
            .post(request.url)
            .header(reqwest::header::AUTHORIZATION, &request.authorization)
            .header("Content-Type", "application/json; charset=utf-8")
            .header("Host", "cvm.tencentcloudapi.com")
            .body(request.payload)
            .send()
            .await
            .map_err(|_| ConnectorFailure {
                code: ErrorCode::NetworkUnreachable,
                message: "Tencent network error".into(),
                retryable: true,
                retry_after_ms: None,
            })?;

        let status = response.status();
        if status.as_u16() == 429 {
            let retry_after_ms = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|h| h.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .map(|s| s.saturating_mul(1000));
            return Err(ConnectorFailure {
                code: ErrorCode::RateLimited,
                message: "Tencent rate limited".into(),
                retryable: true,
                retry_after_ms,
            });
        }
        if status.as_u16() == 403 {
            return Err(ConnectorFailure {
                code: ErrorCode::AuthenticationFailed,
                message: "Tencent authentication failed".into(),
                retryable: false,
                retry_after_ms: None,
            });
        }
        if !status.is_success() {
            return Err(ConnectorFailure {
                code: ErrorCode::ProviderUnavailable,
                message: format!("Tencent API returned {}", status.as_u16()),
                retryable: false,
                retry_after_ms: None,
            });
        }

        let body = response
            .bytes()
            .await
            .map_err(|_| ConnectorFailure {
                code: ErrorCode::NetworkUnreachable,
                message: "Tencent network error".into(),
                retryable: true,
                retry_after_ms: None,
            })?
            .to_vec();
        Ok(body)
    }
}

// ---------------------------------------------------------------------------
// Connector builders
// ---------------------------------------------------------------------------

fn build_validation_request(provider: Provider, config: serde_json::Value) -> ValidationRequest {
    ValidationRequest {
        connection: ConnectionInput {
            connection_id: ConnectionId::new(format!("live-smoke-{}", provider.name()))
                .expect("valid connection id"),
            connector_type: provider.connector_type(),
            config,
            config_schema_version: SchemaVersion::new(1).unwrap(),
        },
    }
}

fn build_sync_request_with_scope(
    provider: Provider,
    config: serde_json::Value,
    scope: &str,
) -> SyncRequest {
    SyncRequest {
        sync_run_id: SyncRunId::new(format!("live-smoke-run-{}", provider.name()))
            .expect("valid sync run id"),
        connection: ConnectionInput {
            connection_id: ConnectionId::new(format!("live-smoke-{}", provider.name()))
                .expect("valid connection id"),
            connector_type: provider.connector_type(),
            config,
            config_schema_version: SchemaVersion::new(1).unwrap(),
        },
        mode: SyncMode::Full,
        scope: Scope::new(scope).expect("valid scope"),
        cursor: None,
        targeted_resources: vec![],
    }
}

fn build_sync_request(provider: Provider, config: serde_json::Value) -> SyncRequest {
    SyncRequest {
        sync_run_id: SyncRunId::new(format!("live-smoke-run-{}", provider.name()))
            .expect("valid sync run id"),
        connection: ConnectionInput {
            connection_id: ConnectionId::new(format!("live-smoke-{}", provider.name()))
                .expect("valid connection id"),
            connector_type: provider.connector_type(),
            config,
            config_schema_version: SchemaVersion::new(1).unwrap(),
        },
        mode: SyncMode::Full,
        scope: Scope::new(format!("live-smoke-{}", provider.name())).expect("valid scope"),
        cursor: None,
        targeted_resources: vec![],
    }
}

// ---------------------------------------------------------------------------
// Safe summary
// ---------------------------------------------------------------------------

struct SafeSummary {
    provider: Provider,
    elapsed_ms: u64,
    validation_status: String,
    validation_errors: Vec<String>,
    sync_status: String,
    resource_count: usize,
    relation_count: usize,
    status_class_counts: BTreeMap<String, u64>,
    coverage: String,
    conformance_issues: Vec<String>,
}

impl SafeSummary {
    fn print(&self) {
        println!("=== Live Smoke Summary: {} ===", self.provider.name());
        println!("elapsed_ms: {}", self.elapsed_ms);
        println!("validation_status: {}", self.validation_status);
        if !self.validation_errors.is_empty() {
            println!("validation_errors:");
            for e in &self.validation_errors {
                println!("  - {}", e);
            }
        }
        println!("sync_status: {}", self.sync_status);
        println!("resource_count: {}", self.resource_count);
        println!("relation_count: {}", self.relation_count);
        if !self.status_class_counts.is_empty() {
            println!("status_class_counts:");
            for (cls, cnt) in &self.status_class_counts {
                println!("  {}xx: {}", cls, cnt);
            }
        }
        println!("coverage: {}", self.coverage);
        if !self.conformance_issues.is_empty() {
            println!("conformance_issues:");
            for issue in &self.conformance_issues {
                println!("  - {}", issue);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Dry run
// ---------------------------------------------------------------------------

fn print_dry_run(provider: Option<Provider>) {
    println!("=== Live Smoke Dry Run ===");
    if let Some(p) = provider {
        println!("Provider: {}", p.name());
        println!("Required environment variables:");
        for var in p.required_env_vars() {
            println!("  {}", var);
        }
    } else {
        println!("All providers and their required environment variables:");
        for p in Provider::all() {
            println!("\n{}:", p.name());
            for var in p.required_env_vars() {
                println!("  {}", var);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Main run
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    let dry_run = args.iter().any(|a| a == "--dry-run");
    let provider_arg = args.get(1).filter(|a| !a.starts_with("--"));

    let provider = if dry_run {
        provider_arg.and_then(|s| Provider::from_str(s))
    } else {
        provider_arg.and_then(|s| Provider::from_str(s))
    };

    if dry_run {
        print_dry_run(provider);
        return Ok(());
    }

    let provider = provider.ok_or(
        "Usage: next-infra-live-smoke <provider> [--dry-run]\n\
        Providers: dokploy, cloudflare, supabase_managed, supabase_self_hosted, aliyun, tencent\n\
        Use --dry-run to list required env vars",
    )?;

    let started = Instant::now();
    let outcome = run_provider(provider).await;
    let elapsed_ms = started.elapsed().as_millis() as u64;

    let summary = build_summary(&provider, elapsed_ms, &outcome);
    summary.print();

    if !summary.conformance_issues.is_empty() {
        std::process::exit(1);
    }
    if summary.validation_status == "Invalid" {
        std::process::exit(1);
    }

    Ok(())
}

type RunOutcome = Result<(ValidationReport, Option<SyncOutcome>), String>;

async fn run_provider(provider: Provider) -> RunOutcome {
    match provider {
        Provider::Dokploy => run_dokploy().await,
        Provider::Cloudflare => run_cloudflare().await,
        Provider::SupabaseManaged => run_supabase_managed().await,
        Provider::SupabaseSelfHosted => run_supabase_self_hosted().await,
        Provider::Aliyun => run_aliyun().await,
        Provider::Tencent => run_tencent().await,
    }
}

async fn run_dokploy() -> RunOutcome {
    let url =
        std::env::var("NEXT_INFRA_DOKPLOY_URL").map_err(|_| "NEXT_INFRA_DOKPLOY_URL is not set")?;
    let token = std::env::var("NEXT_INFRA_DOKPLOY_TOKEN")
        .map_err(|_| "NEXT_INFRA_DOKPLOY_TOKEN is not set")?;

    let transport = LiveDokployTransport::new();
    let connector = DokployConnector::new(&url, transport)
        .map_err(|e| format!("Failed to create Dokploy connector: {}", e))?;

    let secret = SecretValue::new(token);
    let config = json!({ "base_url": url });

    let validation = connector
        .validate(
            build_validation_request(Provider::Dokploy, config.clone()),
            Some(&secret),
        )
        .await
        .map_err(|e| format!("Dokploy validate failed: {}", e))?;

    if validation.status != ValidationStatus::Valid {
        return Ok((validation, None));
    }

    let sync_outcome = connector
        .sync(build_sync_request(Provider::Dokploy, config), Some(&secret))
        .await
        .map_err(|e| format!("Dokploy sync failed: {}", e))?;

    Ok((validation, Some(sync_outcome)))
}

async fn run_cloudflare() -> RunOutcome {
    let token = std::env::var("NEXT_INFRA_CLOUDFLARE_TOKEN")
        .map_err(|_| "NEXT_INFRA_CLOUDFLARE_TOKEN is not set")?;

    let transport = LiveCloudflareTransport::new();
    let connector = CloudflareConnector::new(transport);

    let secret = SecretValue::new(token);
    let config = json!({});

    let validation = connector
        .validate(
            build_validation_request(Provider::Cloudflare, config.clone()),
            Some(&secret),
        )
        .await
        .map_err(|e| format!("Cloudflare validate failed: {}", e))?;

    if validation.status != ValidationStatus::Valid {
        return Ok((validation, None));
    }

    let sync_outcome = connector
        .sync(
            build_sync_request(Provider::Cloudflare, config),
            Some(&secret),
        )
        .await
        .map_err(|e| format!("Cloudflare sync failed: {}", e))?;

    Ok((validation, Some(sync_outcome)))
}

async fn run_supabase_managed() -> RunOutcome {
    let token = std::env::var("NEXT_INFRA_SUPABASE_MANAGED_TOKEN")
        .map_err(|_| "NEXT_INFRA_SUPABASE_MANAGED_TOKEN is not set")?;

    let transport = LiveManagementTransport::new();
    let connector = SupabaseManagedConnector::new(transport);

    let secret = SecretValue::new(token);
    let config = json!({});

    let validation = connector
        .validate(
            build_validation_request(Provider::SupabaseManaged, config.clone()),
            Some(&secret),
        )
        .await
        .map_err(|e| format!("Supabase managed validate failed: {}", e))?;

    if validation.status != ValidationStatus::Valid {
        return Ok((validation, None));
    }

    let sync_outcome = connector
        .sync(
            build_sync_request(Provider::SupabaseManaged, config),
            Some(&secret),
        )
        .await
        .map_err(|e| format!("Supabase managed sync failed: {}", e))?;

    Ok((validation, Some(sync_outcome)))
}

async fn run_supabase_self_hosted() -> RunOutcome {
    let url = std::env::var("NEXT_INFRA_SUPABASE_SELF_HOSTED_URL")
        .map_err(|_| "NEXT_INFRA_SUPABASE_SELF_HOSTED_URL is not set")?;
    let service_key = std::env::var("NEXT_INFRA_SUPABASE_SELF_HOSTED_SERVICE_KEY")
        .map_err(|_| "NEXT_INFRA_SUPABASE_SELF_HOSTED_SERVICE_KEY is not set")?;

    let transport = LiveSelfHostedTransport::new(&url, &service_key)
        .map_err(|e| format!("Failed to create Supabase self-hosted transport: {}", e))?;
    let connector = SupabaseSelfHostedConnector::new(transport);

    let secret = SecretValue::new(service_key);
    let config = json!({ "base_url": url });

    let validation = connector
        .validate(
            build_validation_request(Provider::SupabaseSelfHosted, config.clone()),
            Some(&secret),
        )
        .await
        .map_err(|e| format!("Supabase self-hosted validate failed: {}", e))?;

    if validation.status != ValidationStatus::Valid {
        return Ok((validation, None));
    }

    let sync_outcome = connector
        .sync(
            build_sync_request(Provider::SupabaseSelfHosted, config),
            Some(&secret),
        )
        .await
        .map_err(|e| format!("Supabase self-hosted sync failed: {}", e))?;

    Ok((validation, Some(sync_outcome)))
}

async fn run_aliyun() -> RunOutcome {
    let access_key_id = std::env::var("NEXT_INFRA_ALIYUN_ACCESS_KEY_ID")
        .map_err(|_| "NEXT_INFRA_ALIYUN_ACCESS_KEY_ID is not set")?;
    let access_key_secret = std::env::var("NEXT_INFRA_ALIYUN_ACCESS_KEY_SECRET")
        .map_err(|_| "NEXT_INFRA_ALIYUN_ACCESS_KEY_SECRET is not set")?;
    let region = std::env::var("NEXT_INFRA_ALIYUN_REGION").unwrap_or_else(|_| "cn-hangzhou".into());

    let transport = LiveAliyunTransport::new();
    let connector = AliyunConnector::new(transport);

    let secret = SecretValue::new(access_key_secret);
    let config = json!({ "access_key_id": access_key_id });

    let validation = connector
        .validate(
            build_validation_request(Provider::Aliyun, config.clone()),
            Some(&secret),
        )
        .await
        .map_err(|e| format!("Aliyun validate failed: {}", e))?;

    if validation.status != ValidationStatus::Valid {
        return Ok((validation, None));
    }

    let sync_outcome = connector
        .sync(
            build_sync_request_with_scope(Provider::Aliyun, config, &format!("aliyun:{region}")),
            Some(&secret),
        )
        .await
        .map_err(|e| format!("Aliyun sync failed: {}", e))?;

    Ok((validation, Some(sync_outcome)))
}

async fn run_tencent() -> RunOutcome {
    let secret_id = std::env::var("NEXT_INFRA_TENCENT_SECRET_ID")
        .map_err(|_| "NEXT_INFRA_TENCENT_SECRET_ID is not set")?;
    let secret_key = std::env::var("NEXT_INFRA_TENCENT_SECRET_KEY")
        .map_err(|_| "NEXT_INFRA_TENCENT_SECRET_KEY is not set")?;
    let region =
        std::env::var("NEXT_INFRA_TENCENT_REGION").unwrap_or_else(|_| "ap-guangzhou".into());

    let transport = LiveTencentTransport::new();
    let connector = TencentConnector::new(transport);

    let secret = SecretValue::new(secret_key);
    let config = json!({ "secret_id": secret_id });

    let validation = connector
        .validate(
            build_validation_request(Provider::Tencent, config.clone()),
            Some(&secret),
        )
        .await
        .map_err(|e| format!("Tencent validate failed: {}", e))?;

    if validation.status != ValidationStatus::Valid {
        return Ok((validation, None));
    }

    let sync_outcome = connector
        .sync(
            build_sync_request_with_scope(Provider::Tencent, config, &format!("tencent:{region}")),
            Some(&secret),
        )
        .await
        .map_err(|e| format!("Tencent sync failed: {}", e))?;

    Ok((validation, Some(sync_outcome)))
}

fn build_summary(provider: &Provider, elapsed_ms: u64, outcome: &RunOutcome) -> SafeSummary {
    let (validation, sync_outcome) = match outcome {
        Ok((v, s)) => (v, s),
        Err(e) => {
            return SafeSummary {
                provider: *provider,
                elapsed_ms,
                validation_status: "Error".to_string(),
                validation_errors: vec![e.clone()],
                sync_status: "Skipped".to_string(),
                resource_count: 0,
                relation_count: 0,
                status_class_counts: BTreeMap::new(),
                coverage: "N/A".to_string(),
                conformance_issues: vec![e.clone()],
            };
        }
    };

    let validation_status = format!("{:?}", validation.status);
    let validation_errors: Vec<String> = validation
        .errors
        .iter()
        .map(|e| format!("{:?}: {}", e.code, e.message))
        .collect();

    let (sync_status, resource_count, relation_count, status_class_counts, coverage, issues) =
        match sync_outcome {
            None => (
                "Skipped".to_string(),
                0usize,
                0usize,
                BTreeMap::new(),
                "N/A".to_string(),
                vec![],
            ),
            Some(outcome) => {
                let sync_status = match outcome {
                    SyncOutcome::Complete { .. } => "Complete",
                    SyncOutcome::Partial { .. } => "Partial",
                }
                .to_string();

                let batch = outcome.batch();
                let resource_count = batch.resources.len();
                let relation_count = batch.relations.len();

                let status_class_counts =
                    batch.provider_request_summary.status_class_counts.clone();

                let coverage = format!("{:?}", batch.coverage);

                let mut issues = check_outcome(&build_sync_request(*provider, json!({})), outcome);
                issues.extend(check_batch(batch));
                let issues: Vec<String> = issues
                    .iter()
                    .map(|i| format!("{:?}: {}", i.code, i.message))
                    .collect();

                (
                    sync_status,
                    resource_count,
                    relation_count,
                    status_class_counts,
                    coverage,
                    issues,
                )
            }
        };

    SafeSummary {
        provider: *provider,
        elapsed_ms,
        validation_status,
        validation_errors,
        sync_status,
        resource_count,
        relation_count,
        status_class_counts,
        coverage,
        conformance_issues: issues,
    }
}
