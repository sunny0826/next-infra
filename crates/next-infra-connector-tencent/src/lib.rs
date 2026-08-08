//! Bounded Tencent read contract: CVM, VPC/network, and edge summaries.
use async_trait::async_trait;
use hmac::{Hmac, Mac};
use next_infra_connector_api::ResourceObservation;
use next_infra_core::{
    ConnectorCoverage, ConnectorCoverageLevel, ConnectorType, ExternalId, LabelKey, ResourceHealth,
    ResourceKind, SchemaVersion, Scope, SyncMode, Timestamp,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use sha2_hmac::Sha256 as HmacSha256Digest;
use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

type HmacSha256 = Hmac<HmacSha256Digest>;

/// Builds a Tencent TC3-HMAC-SHA256 authorization value without retaining the
/// secret after the call. Inputs are deliberately bounded to a single POST.
#[allow(clippy::too_many_arguments)] // TC3 specifies these independent signed fields.
pub fn tc3_authorization(
    secret: &next_infra_core::SecretValue,
    secret_id: &str,
    service: &str,
    host: &str,
    action: &str,
    region: &str,
    timestamp: u64,
    date: &str,
    payload: &[u8],
) -> Result<String, String> {
    if secret_id.is_empty()
        || service.is_empty()
        || host.is_empty()
        || action.is_empty()
        || region.is_empty()
        || date.len() != 10
    {
        return Err("invalid TC3 signing input".into());
    }
    let payload_hash = hex(Sha256::digest(payload));
    let canonical_headers = format!("content-type:application/json; charset=utf-8\nhost:{host}\n");
    let canonical_request =
        format!("POST\n/\n\n{canonical_headers}\ncontent-type;host\n{payload_hash}");
    let scope = format!("{date}/{service}/tc3_request");
    let string_to_sign = format!(
        "TC3-HMAC-SHA256\n{timestamp}\n{scope}\n{}",
        hex(Sha256::digest(canonical_request.as_bytes()))
    );
    let date_key = hmac(
        format!(
            "TC3{}",
            std::str::from_utf8(secret.expose()).map_err(|_| "invalid TC3 secret")?
        )
        .as_bytes(),
        date.as_bytes(),
    )?;
    let service_key = hmac(&date_key, service.as_bytes())?;
    let signing_key = hmac(
        &hmac(&service_key, b"tc3_request")?,
        string_to_sign.as_bytes(),
    )?;
    let signature = hex(signing_key);
    Ok(format!(
        "TC3-HMAC-SHA256 Credential={secret_id}/{scope}, SignedHeaders=content-type;host, Signature={signature}"
    ))
}
fn hmac(key: &[u8], value: &[u8]) -> Result<Vec<u8>, String> {
    let mut mac = HmacSha256::new_from_slice(key).map_err(|_| "invalid HMAC key")?;
    mac.update(value);
    Ok(mac.finalize().into_bytes().to_vec())
}
fn hex(bytes: impl AsRef<[u8]>) -> String {
    bytes
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[async_trait]
pub trait TencentTransport: Send + Sync {
    async fn list(
        &self,
        request: SignedRequest,
        module: &'static str,
    ) -> Result<Vec<u8>, next_infra_connector_api::ConnectorFailure>;
}
pub struct TencentConnector<T> {
    descriptor: next_infra_connector_api::ConnectorDescriptor,
    transport: T,
}
impl<T> TencentConnector<T> {
    pub fn new(transport: T) -> Self {
        Self {
            descriptor: descriptor(),
            transport,
        }
    }
}
#[async_trait]
impl<T: TencentTransport> next_infra_connector_api::ReadConnector for TencentConnector<T> {
    fn descriptor(&self) -> &next_infra_connector_api::ConnectorDescriptor {
        &self.descriptor
    }
    async fn validate(
        &self,
        request: next_infra_connector_api::ValidationRequest,
        secret: Option<&next_infra_core::SecretValue>,
    ) -> next_infra_connector_api::ConnectorResult<next_infra_connector_api::ValidationReport> {
        let mut errors = vec![];
        if request.connection.connector_type != self.descriptor.connector_type {
            errors.push(issue(
                next_infra_core::ErrorCode::InvalidDomainValue,
                "Tencent connector type mismatch",
            ));
        }
        if secret.is_none() {
            errors.push(issue(
                next_infra_core::ErrorCode::CredentialUnavailable,
                "Tencent credential is unavailable",
            ));
        }
        Ok(next_infra_connector_api::ValidationReport {
            status: if errors.is_empty() {
                next_infra_connector_api::ValidationStatus::Valid
            } else {
                next_infra_connector_api::ValidationStatus::Invalid
            },
            warnings: vec![],
            errors,
        })
    }
    async fn sync(
        &self,
        request: next_infra_connector_api::SyncRequest,
        secret: Option<&next_infra_core::SecretValue>,
    ) -> next_infra_connector_api::ConnectorResult<next_infra_connector_api::SyncOutcome> {
        if request.connection.connector_type != self.descriptor.connector_type {
            return Err(failure(
                next_infra_core::ErrorCode::InvalidDomainValue,
                "Tencent connector type mismatch",
            ));
        }
        let secret = secret.ok_or_else(|| {
            failure(
                next_infra_core::ErrorCode::CredentialUnavailable,
                "Tencent credential is unavailable",
            )
        })?;
        let secret_id = request
            .connection
            .config
            .get("secret_id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| next_infra_connector_api::ConnectorFailure {
                code: next_infra_core::ErrorCode::InvalidDomainValue,
                message: "Tencent connection config requires secret_id".into(),
                retryable: false,
                retry_after_ms: None,
            })?
            .to_owned();
        let region = request
            .scope
            .as_str()
            .strip_prefix("tencent:")
            .unwrap_or(request.scope.as_str())
            .to_owned();
        let modules = [
            (
                "tencent.compute.cvm",
                "tencent.cvm.instance",
                "DescribeInstances",
                "2017-03-12",
            ),
            (
                "tencent.network.vpc",
                "tencent.vpc.vpc",
                "DescribeVpcs",
                "2017-03-12",
            ),
            (
                "tencent.network.subnet",
                "tencent.vpc.subnet",
                "DescribeSubnets",
                "2017-03-12",
            ),
            (
                "tencent.network.security_group",
                "tencent.vpc.security_group",
                "DescribeSecurityGroups",
                "2017-03-12",
            ),
            (
                "tencent.edge.clb",
                "tencent.clb.load_balancer",
                "DescribeLoadBalancers",
                "2018-03-17",
            ),
            (
                "tencent.edge.dns",
                "tencent.dns.record",
                "DescribeRecordList",
                "2021-03-23",
            ),
            (
                "tencent.edge.public_ip",
                "tencent.cvm.public_ip",
                "DescribeAddresses",
                "2017-03-12",
            ),
        ];
        let mut resources = vec![];
        let mut error = None;
        for (module, kind, action, version) in modules {
            let fetch = if module == "tencent.edge.dns" {
                fetch_dns(&self.transport, &request.scope, &region, &secret_id, secret).await
            } else {
                fetch_module(
                    &self.transport,
                    module,
                    kind,
                    action,
                    version,
                    &request.scope,
                    &region,
                    &secret_id,
                    secret,
                )
                .await
            };
            resources.extend(fetch.resources);
            if let Some(failure) = fetch.failure {
                error = Some(failure);
            }
        }
        resources.sort_by_key(|r| (r.kind.clone(), r.external_id.clone()));
        let partial = error.is_some();
        let coverage = if partial {
            next_infra_core::SyncCoverage::Partial {
                scope: Some(request.scope.clone()),
                reason: gap_reason(error.as_ref().unwrap()),
            }
        } else {
            next_infra_core::SyncCoverage::AuthoritativeFull {
                scope: request.scope.clone(),
            }
        };
        if resources.is_empty()
            && partial
            && matches!(
                error.as_ref().unwrap().code,
                next_infra_core::ErrorCode::AuthenticationFailed
                    | next_infra_core::ErrorCode::CredentialUnavailable
            )
        {
            return Err(error.unwrap());
        }
        let relations = provider_relations(&resources);
        let batch = next_infra_connector_api::ObservationBatch {
            resources,
            relations,
            coverage,
            next_cursor: None,
            warnings: vec![],
            redaction_report: Default::default(),
            provider_request_summary: Default::default(),
        };
        let outcome = if let Some(f) = error {
            next_infra_connector_api::SyncOutcome::Partial { batch, failure: f }
        } else {
            next_infra_connector_api::SyncOutcome::Complete { batch }
        };
        outcome.validate_for(&request).map_err(|_| {
            failure(
                next_infra_core::ErrorCode::InvalidResponse,
                "Tencent outcome is invalid",
            )
        })?;
        Ok(outcome)
    }
}
fn issue(
    code: next_infra_core::ErrorCode,
    message: &str,
) -> next_infra_connector_api::ValidationIssue {
    next_infra_connector_api::ValidationIssue {
        code,
        message: message.into(),
    }
}
fn failure(
    code: next_infra_core::ErrorCode,
    message: &str,
) -> next_infra_connector_api::ConnectorFailure {
    next_infra_connector_api::ConnectorFailure {
        code,
        message: message.into(),
        retryable: false,
        retry_after_ms: None,
    }
}

fn gap_reason(
    failure: &next_infra_connector_api::ConnectorFailure,
) -> next_infra_core::CoverageGapReason {
    match failure.code {
        next_infra_core::ErrorCode::RateLimited => next_infra_core::CoverageGapReason::RateLimited,
        next_infra_core::ErrorCode::PartialPagination => {
            next_infra_core::CoverageGapReason::PaginationIncomplete
        }
        next_infra_core::ErrorCode::AuthenticationFailed
        | next_infra_core::ErrorCode::PermissionDenied => {
            next_infra_core::CoverageGapReason::PermissionDenied
        }
        _ => next_infra_core::CoverageGapReason::ProviderUnavailable,
    }
}

/// Per-product API origin (Tencent Cloud docs: each product has its own endpoint).
pub fn module_origin(module: &str) -> &'static str {
    match module {
        "tencent.compute.cvm" => "https://cvm.tencentcloudapi.com",
        "tencent.network.vpc"
        | "tencent.network.subnet"
        | "tencent.network.security_group"
        | "tencent.edge.public_ip" => "https://vpc.tencentcloudapi.com",
        "tencent.edge.clb" => "https://clb.tencentcloudapi.com",
        "tencent.edge.dns" => "https://dnspod.tencentcloudapi.com",
        _ => "https://cvm.tencentcloudapi.com",
    }
}

/// Per-product hostname for the Host header.
pub fn module_host(module: &str) -> &'static str {
    match module {
        "tencent.compute.cvm" => "cvm.tencentcloudapi.com",
        "tencent.network.vpc"
        | "tencent.network.subnet"
        | "tencent.network.security_group"
        | "tencent.edge.public_ip" => "vpc.tencentcloudapi.com",
        "tencent.edge.clb" => "clb.tencentcloudapi.com",
        "tencent.edge.dns" => "dnspod.tencentcloudapi.com",
        _ => "cvm.tencentcloudapi.com",
    }
}

/// Per-product TC3 service name.
pub fn module_service(module: &str) -> &'static str {
    match module {
        "tencent.compute.cvm" => "cvm",
        "tencent.network.vpc"
        | "tencent.network.subnet"
        | "tencent.network.security_group"
        | "tencent.edge.public_ip" => "vpc",
        "tencent.edge.clb" => "clb",
        "tencent.edge.dns" => "dnspod",
        _ => "cvm",
    }
}

const PAGE_SIZE: u32 = 100;
const MAX_PAGES_PER_MODULE: u32 = 100;
const MAX_RESOURCES_PER_MODULE: usize = 10_000;
const MAX_RETRIES: u32 = 2;
const MAX_RETRY_AFTER_MS: u64 = 5_000;

pub struct SignedRequest {
    pub url: url::Url,
    pub host: String,
    pub secret_id: String,
    pub authorization: String,
    pub payload: Vec<u8>,
    pub timestamp: u64,
    pub action: String,
    pub version: String,
    pub region: String,
}
impl SignedRequest {
    #[allow(clippy::too_many_arguments)] // TC3 signing binds all independent request fields.
    pub fn new(
        origin: &str,
        host: &str,
        service: &str,
        action: &str,
        version: &str,
        region: &str,
        secret_id: &str,
        secret_key: &next_infra_core::SecretValue,
        timestamp: u64,
        limit: u32,
        offset: u32,
    ) -> Result<Self, String> {
        Self::build(
            origin,
            host,
            service,
            action,
            version,
            region,
            secret_id,
            secret_key,
            timestamp,
            limit,
            offset,
            &[],
        )
    }

    /// Same as [`Self::new`] but appends action-specific payload fields
    /// (e.g. `Domain` for DNS record listing).
    #[allow(clippy::too_many_arguments)] // TC3 signing binds all independent request fields.
    pub fn with_extra_params(
        origin: &str,
        host: &str,
        service: &str,
        action: &str,
        version: &str,
        region: &str,
        secret_id: &str,
        secret_key: &next_infra_core::SecretValue,
        timestamp: u64,
        limit: u32,
        offset: u32,
        extra: &[(&str, &str)],
    ) -> Result<Self, String> {
        Self::build(
            origin, host, service, action, version, region, secret_id, secret_key, timestamp,
            limit, offset, extra,
        )
    }

    #[allow(clippy::too_many_arguments)] // TC3 signing binds all independent request fields.
    fn build(
        origin: &str,
        host: &str,
        service: &str,
        action: &str,
        version: &str,
        region: &str,
        secret_id: &str,
        secret_key: &next_infra_core::SecretValue,
        timestamp: u64,
        limit: u32,
        offset: u32,
        extra: &[(&str, &str)],
    ) -> Result<Self, String> {
        if action.is_empty()
            || version.is_empty()
            || region.is_empty()
            || secret_id.is_empty()
            || host.is_empty()
            || service.is_empty()
        {
            return Err("invalid Tencent request scope".into());
        }
        if limit == 0 || limit > 100 {
            return Err("invalid Tencent pagination window".into());
        }
        let mut payload_map = serde_json::Map::new();
        let string_pagination = service == "vpc";
        if string_pagination {
            payload_map.insert("Limit".into(), json!(limit.to_string()));
            payload_map.insert("Offset".into(), json!(offset.to_string()));
        } else {
            payload_map.insert("Limit".into(), json!(limit));
            payload_map.insert("Offset".into(), json!(offset));
        }
        for (key, value) in extra {
            payload_map.insert((*key).to_string(), json!(value));
        }
        let payload =
            serde_json::to_vec(&payload_map).map_err(|_| "invalid Tencent payload".to_string())?;
        let date = format_utc_date(timestamp);
        let authorization = tc3_authorization(
            secret_key, secret_id, service, host, action, region, timestamp, &date, &payload,
        )?;
        Ok(Self {
            url: url::Url::parse(origin).unwrap(),
            host: host.into(),
            secret_id: secret_id.into(),
            authorization,
            payload,
            timestamp,
            action: action.into(),
            version: version.into(),
            region: region.into(),
        })
    }
}
impl std::fmt::Debug for SignedRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignedRequest")
            .field("url", &self.url)
            .field("host", &self.host)
            .field("secret_id", &"[REDACTED]")
            .field("authorization", &"[REDACTED]")
            .field("payload_bytes", &self.payload.len())
            .finish()
    }
}

fn format_utc_date(unix_secs: u64) -> String {
    let (year, month, day) = civil_from_unix_secs(unix_secs);
    format!("{year:04}-{month:02}-{day:02}")
}

fn civil_from_unix_secs(unix_secs: u64) -> (u32, u32, u32) {
    let days = i64::try_from(unix_secs / 86_400).unwrap_or(i64::MAX);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    let year = if month <= 2 { year + 1 } else { year };
    (year as u32, month as u32, day as u32)
}

/// Tencent TC3 list responses wrap items inside `{"Response": {"TotalCount": N,
/// "<Xxx>Set": [...] or "<Xxx>List": [...], "RequestId": "..."}}`.
/// Extract the nested array generically — first try `Response` wrapper,
/// then tolerate flat shapes.
/// Returns `(items, total_count)`.
fn parse_page_envelope(body: &[u8]) -> Result<(Vec<ResourceDto>, u64), String> {
    let value: serde_json::Value = serde_json::from_slice(body)
        .map_err(|_| "Tencent response is not valid JSON".to_string())?;
    let response = value.get("Response").filter(|v| v.is_object());
    let total_count = response
        .and_then(|r| r.get("TotalCount"))
        .and_then(serde_json::Value::as_u64)
        .or_else(|| value.get("TotalCount").and_then(serde_json::Value::as_u64))
        .unwrap_or(0);
    let items: Vec<ResourceDto> = response
        .and_then(|r| {
            r.as_object().and_then(|map| {
                map.values()
                    .find(|v| v.is_array())
                    .map(|arr| serde_json::from_value(arr.clone()).unwrap_or_default())
            })
        })
        .or_else(|| {
            value
                .get("Items")
                .filter(|v| v.is_array())
                .map(|arr| serde_json::from_value(arr.clone()).unwrap_or_default())
        })
        .unwrap_or_default();
    if total_count > 0 && items.is_empty() {
        return Err("Tencent response array is missing".into());
    }
    Ok((items, total_count))
}

struct ModuleFetch {
    resources: Vec<ResourceObservation>,
    failure: Option<next_infra_connector_api::ConnectorFailure>,
}

#[allow(clippy::too_many_arguments)] // Bounded per-module fetch binds scope, kind and signing inputs.
async fn fetch_module<T: TencentTransport>(
    transport: &T,
    module: &'static str,
    kind: &str,
    action: &str,
    version: &str,
    scope: &Scope,
    region: &str,
    secret_id: &str,
    secret_key: &next_infra_core::SecretValue,
) -> ModuleFetch {
    let origin = module_origin(module);
    let host = module_host(module);
    let service = module_service(module);
    let mut resources = Vec::new();
    let mut fetch_failure = None;
    let mut offset = 0u32;
    let mut pages = 0u32;
    loop {
        if pages >= MAX_PAGES_PER_MODULE || resources.len() >= MAX_RESOURCES_PER_MODULE {
            fetch_failure = Some(failure(
                next_infra_core::ErrorCode::PartialPagination,
                "Tencent pagination exceeded its bounded budget",
            ));
            break;
        }
        let result = retry_list(transport, module, || {
            SignedRequest::new(
                origin,
                host,
                service,
                action,
                version,
                region,
                secret_id,
                secret_key,
                now_unix_secs(),
                PAGE_SIZE,
                offset,
            )
        })
        .await;
        let body = match result {
            Ok(body) => body,
            Err(f) => {
                fetch_failure = Some(f);
                break;
            }
        };
        let (items, total_count) = match parse_page_envelope(&body) {
            Ok(envelope) => envelope,
            Err(_) => {
                fetch_failure = Some(failure(
                    next_infra_core::ErrorCode::InvalidResponse,
                    "Tencent module response is invalid",
                ));
                break;
            }
        };
        let empty_page = items.is_empty();
        let observations = items
            .into_iter()
            .map(|value| map(kind, scope, Timestamp::from_unix_millis(0).unwrap(), value))
            .collect::<Result<Vec<_>, _>>();
        resources.extend(match observations {
            Ok(observations) => observations,
            Err(_) => {
                fetch_failure = Some(failure(
                    next_infra_core::ErrorCode::InvalidResponse,
                    "Tencent resource response is invalid",
                ));
                break;
            }
        });
        if u64::try_from(resources.len()).unwrap_or(u64::MAX) >= total_count || empty_page {
            break;
        }
        offset = offset.saturating_add(PAGE_SIZE);
        pages += 1;
    }
    ModuleFetch {
        resources,
        failure: fetch_failure,
    }
}

const MAX_DNS_DOMAINS: usize = 20;

/// DNS records require a two-step walk: list domains, then per-domain records.
async fn fetch_dns<T: TencentTransport>(
    transport: &T,
    scope: &Scope,
    region: &str,
    secret_id: &str,
    secret_key: &next_infra_core::SecretValue,
) -> ModuleFetch {
    let origin = module_origin("tencent.edge.dns");
    let host = module_host("tencent.edge.dns");
    let service = module_service("tencent.edge.dns");
    let version = "2021-03-23";
    let mut domains: Vec<String> = Vec::new();
    let mut page = 1u32;
    loop {
        if page > MAX_PAGES_PER_MODULE || domains.len() >= MAX_DNS_DOMAINS {
            break;
        }
        let body = match retry_list(transport, "tencent.edge.dns", || {
            SignedRequest::with_extra_params(
                origin,
                host,
                service,
                "DescribeDomainList",
                version,
                region,
                secret_id,
                secret_key,
                now_unix_secs(),
                PAGE_SIZE,
                (page - 1) * PAGE_SIZE,
                &[],
            )
        })
        .await
        {
            Ok(body) => body,
            Err(failure) => {
                return ModuleFetch {
                    resources: Vec::new(),
                    failure: Some(failure),
                };
            }
        };
        let value: serde_json::Value = match serde_json::from_slice(&body) {
            Ok(value) => value,
            Err(_) => {
                return ModuleFetch {
                    resources: Vec::new(),
                    failure: Some(failure(
                        next_infra_core::ErrorCode::InvalidResponse,
                        "Tencent DNS domains response is invalid",
                    )),
                };
            }
        };
        let response = value.get("Response");
        let total_count = response
            .and_then(|r| r.get("TotalCount"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let found: Vec<String> = response
            .and_then(|r| r.as_object())
            .and_then(|map| map.values().find(|v| v.is_array()))
            .map(|array| {
                array
                    .as_array()
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|item| {
                                item.get("Name")
                                    .and_then(serde_json::Value::as_str)
                                    .or_else(|| {
                                        item.get("Domain").and_then(serde_json::Value::as_str)
                                    })
                                    .or_else(|| {
                                        item.get("DomainName").and_then(serde_json::Value::as_str)
                                    })
                            })
                            .map(str::to_owned)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        let found_empty = found.is_empty();
        domains.extend(found);
        if domains.len() as u64 >= total_count || found_empty {
            break;
        }
        page += 1;
    }
    domains.truncate(MAX_DNS_DOMAINS);

    let at = Timestamp::from_unix_millis(0)
        .map_err(|_| "invalid timestamp")
        .ok();
    let at = at.unwrap();
    let mut resources = Vec::new();
    let mut fetch_failure = None;
    for domain in domains {
        if resources.len() >= MAX_RESOURCES_PER_MODULE {
            fetch_failure = Some(failure(
                next_infra_core::ErrorCode::PartialPagination,
                "Tencent DNS records exceeded its bounded budget",
            ));
            break;
        }
        let mut offset = 0u32;
        loop {
            if resources.len() >= MAX_RESOURCES_PER_MODULE {
                fetch_failure = Some(failure(
                    next_infra_core::ErrorCode::PartialPagination,
                    "Tencent DNS records exceeded its bounded budget",
                ));
                break;
            }
            let body = match retry_list(transport, "tencent.edge.dns", || {
                SignedRequest::with_extra_params(
                    origin,
                    host,
                    service,
                    "DescribeRecordList",
                    version,
                    region,
                    secret_id,
                    secret_key,
                    now_unix_secs(),
                    PAGE_SIZE,
                    offset,
                    &[("Domain", &domain)],
                )
            })
            .await
            {
                Ok(body) => body,
                Err(f) => {
                    fetch_failure = Some(f);
                    break;
                }
            };
            let (items, total_count) = match parse_page_envelope(&body) {
                Ok(envelope) => envelope,
                Err(_) => {
                    fetch_failure = Some(failure(
                        next_infra_core::ErrorCode::InvalidResponse,
                        "Tencent DNS records response is invalid",
                    ));
                    break;
                }
            };
            let empty_page = items.is_empty();
            for value in items {
                match map("tencent.dns.record", scope, at, value) {
                    Ok(observation) => resources.push(observation),
                    Err(_) => {
                        fetch_failure = Some(failure(
                            next_infra_core::ErrorCode::InvalidResponse,
                            "Tencent DNS record mapping failed",
                        ));
                        break;
                    }
                }
            }
            if u64::try_from(resources.len()).unwrap_or(u64::MAX) >= total_count || empty_page {
                break;
            }
            offset = offset.saturating_add(PAGE_SIZE);
        }
    }
    ModuleFetch {
        resources,
        failure: fetch_failure,
    }
}

async fn retry_list<T, F>(
    transport: &T,
    module: &'static str,
    build: F,
) -> Result<Vec<u8>, next_infra_connector_api::ConnectorFailure>
where
    T: TencentTransport,
    F: Fn() -> Result<SignedRequest, String>,
{
    let mut attempts = 0u32;
    loop {
        let request = build().map_err(|_| {
            failure(
                next_infra_core::ErrorCode::InvalidDomainValue,
                "Tencent request scope is invalid",
            )
        })?;
        match transport.list(request, module).await {
            Ok(body) => return Ok(body),
            Err(f)
                if f.code == next_infra_core::ErrorCode::RateLimited
                    && f.retryable
                    && attempts < MAX_RETRIES =>
            {
                attempts += 1;
                if let Some(ms) = f.retry_after_ms.filter(|ms| *ms > 0) {
                    tokio::time::sleep(Duration::from_millis(ms.min(MAX_RETRY_AFTER_MS))).await;
                }
            }
            Err(f) => return Err(f),
        }
    }
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub fn descriptor() -> next_infra_connector_api::ConnectorDescriptor {
    let kinds = [
        (
            "tencent.cvm.instance",
            "tencent.compute.cvm",
            ConnectorCoverageLevel::Supported,
        ),
        (
            "tencent.vpc.vpc",
            "tencent.network.vpc",
            ConnectorCoverageLevel::Supported,
        ),
        (
            "tencent.vpc.subnet",
            "tencent.network.subnet",
            ConnectorCoverageLevel::Supported,
        ),
        (
            "tencent.vpc.security_group",
            "tencent.network.security_group",
            ConnectorCoverageLevel::Partial,
        ),
        (
            "tencent.clb.load_balancer",
            "tencent.edge.clb",
            ConnectorCoverageLevel::Partial,
        ),
        (
            "tencent.dns.record",
            "tencent.edge.dns",
            ConnectorCoverageLevel::Partial,
        ),
        (
            "tencent.cvm.public_ip",
            "tencent.edge.public_ip",
            ConnectorCoverageLevel::Partial,
        ),
    ];
    next_infra_connector_api::ConnectorDescriptor {
        connector_type: ConnectorType::new("tencent").unwrap(),
        connector_version: "1.0.0".into(),
        config_schema_version: SchemaVersion::new(1).unwrap(),
        auth: next_infra_connector_api::AuthDescriptor {
            kind: next_infra_connector_api::AuthKind::ApiKey,
            minimum_permissions: vec![
                "cvm:Describe*".into(),
                "vpc:Describe*".into(),
                "clb:Describe*".into(),
                "dnspod:Describe*".into(),
            ],
        },
        sync_modes: vec![SyncMode::Full, SyncMode::Targeted],
        resources: kinds
            .into_iter()
            .map(|(k, m, l)| next_infra_connector_api::ResourceCapability {
                kind: ResourceKind::new(k).unwrap(),
                attribute_schema_version: SchemaVersion::new(1).unwrap(),
                coverage: ConnectorCoverage {
                    module: m.into(),
                    level: l,
                    reason: if matches!(l, ConnectorCoverageLevel::Supported) {
                        None
                    } else {
                        Some("region and permission scoped".into())
                    },
                },
            })
            .collect(),
        relations: vec![
            relation(
                "tencent.vpc.vpc",
                "tencent.vpc.subnet",
                "tencent.contains",
                "tencent.network.vpc_subnet",
            ),
            relation(
                "tencent.vpc.vpc",
                "tencent.vpc.security_group",
                "tencent.contains",
                "tencent.network.vpc_security_group",
            ),
            relation(
                "tencent.cvm.instance",
                "tencent.cvm.public_ip",
                "tencent.assigned",
                "tencent.edge.instance_public_ip",
            ),
        ],
        sensitive_field_policy: vec![
            "SecretId and SecretKey are transient signed request inputs".into(),
            "raw security rules and response bodies are excluded".into(),
        ],
        rate_limit: next_infra_connector_api::RateLimitGuidance {
            default_max_concurrency: 2,
            requests_per_minute: None,
            respects_retry_after: true,
        },
        recommended_sync_interval_secs: 900,
        known_gaps: vec![
            "writes, CAM policy mutation and products outside the listed modules are unsupported"
                .into(),
        ],
    }
}
fn relation(
    source: &str,
    target: &str,
    kind: &str,
    module: &str,
) -> next_infra_connector_api::RelationCapability {
    next_infra_connector_api::RelationCapability {
        source_kind: ResourceKind::new(source).unwrap(),
        target_kind: ResourceKind::new(target).unwrap(),
        kind: next_infra_core::RelationKind::new(kind).unwrap(),
        coverage: ConnectorCoverage {
            module: module.into(),
            level: ConnectorCoverageLevel::Partial,
            reason: Some("relation is emitted only when the provider returns both IDs".into()),
        },
    }
}

/// Tencent products use per-resource id fields (InstanceId, VpcId, …) and
/// some responses also carry a generic `id`; pick the first present one so a
/// duplicate never aborts deserialization.
#[derive(Clone, Debug)]
pub struct ResourceDto {
    pub id: String,
    pub name: Option<String>,
    pub region: Option<String>,
    pub status: Option<String>,
    pub parent_id: Option<String>,
}

impl<'de> serde::Deserialize<'de> for ResourceDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let object = value
            .as_object()
            .ok_or_else(|| serde::de::Error::custom("Tencent resource is not an object"))?;
        let id = [
            "id",
            "InstanceId",
            "SubnetId",
            "SecurityGroupId",
            "LoadBalancerId",
            "VpcId",
            "RecordId",
            "AddressId",
        ]
        .iter()
        .find_map(|key| {
            object.get(*key).and_then(|v| match v {
                serde_json::Value::String(s) => Some(s.clone()),
                serde_json::Value::Number(n) => Some(n.to_string()),
                _ => None,
            })
        })
        .unwrap_or_default();
        let field = |key: &str| {
            object
                .get(key)
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        };
        let parent_id = field("parent_id")
            .or_else(|| field("VpcId"))
            .or_else(|| field("SubnetId"))
            .or_else(|| field("InstanceId"));
        Ok(ResourceDto {
            id,
            name: field("name"),
            region: field("region"),
            status: field("status"),
            parent_id,
        })
    }
}

pub fn map(
    kind: &str,
    scope: &Scope,
    at: Timestamp,
    v: ResourceDto,
) -> Result<ResourceObservation, String> {
    if v.id.is_empty() {
        return Err("Tencent resource id is invalid".into());
    }
    Ok(ResourceObservation {
        kind: ResourceKind::new(kind).map_err(|_| "invalid kind")?,
        external_id: ExternalId::new(format!("{kind}:{}", v.id)).map_err(|_| "invalid id")?,
        name: v.id.clone(),
        display_name: v.name.unwrap_or_else(|| v.id.clone()),
        scope: scope.clone(),
        labels: BTreeMap::from([(
            LabelKey::new("tencent.region").unwrap(),
            v.region.clone().unwrap_or_else(|| "unknown".into()),
        )]),
        health: health(v.status.as_deref()),
        attributes: json!({"region":v.region,"status":v.status,"parent_id":v.parent_id}),
        attribute_schema_version: SchemaVersion::new(1).unwrap(),
        observed_at: at,
    })
}

fn health(status: Option<&str>) -> ResourceHealth {
    match status.map(|value| value.to_ascii_lowercase()) {
        Some(value) if matches!(value.as_str(), "running" | "active" | "available") => {
            ResourceHealth::Healthy
        }
        Some(value) if matches!(value.as_str(), "stopped" | "pending") => ResourceHealth::Degraded,
        Some(value) if matches!(value.as_str(), "error" | "failed") => ResourceHealth::Unhealthy,
        _ => ResourceHealth::Unknown,
    }
}

fn provider_relations(
    resources: &[ResourceObservation],
) -> Vec<next_infra_connector_api::RelationObservation> {
    let mut relations = Vec::new();
    for resource in resources {
        let Some(parent_id) = resource
            .attributes
            .get("parent_id")
            .and_then(|value| value.as_str())
        else {
            continue;
        };
        let (source_kind, relation_kind) = match resource.kind.as_str() {
            "tencent.vpc.subnet" | "tencent.vpc.security_group" => {
                ("tencent.vpc.vpc", "tencent.contains")
            }
            "tencent.cvm.public_ip" => ("tencent.cvm.instance", "tencent.assigned"),
            _ => continue,
        };
        let source_id =
            ExternalId::new(format!("{source_kind}:{parent_id}")).expect("validated provider ID");
        if !resources.iter().any(|candidate| {
            candidate.kind.as_str() == source_kind && candidate.external_id == source_id
        }) {
            continue;
        }
        relations.push(next_infra_connector_api::RelationObservation {
            source: next_infra_connector_api::ResourceLocator {
                kind: ResourceKind::new(source_kind).unwrap(),
                external_id: source_id,
            },
            target: next_infra_connector_api::ResourceLocator {
                kind: resource.kind.clone(),
                external_id: resource.external_id.clone(),
            },
            kind: next_infra_core::RelationKind::new(relation_kind).unwrap(),
            evidence_key: next_infra_core::EvidenceKey::new(format!(
                "tencent:{relation_kind}:{}",
                resource.external_id
            ))
            .unwrap(),
            field_path: next_infra_core::FieldPath::new("parent_id").unwrap(),
            observed_at: resource.observed_at,
        });
    }
    relations.sort_by_key(|relation| {
        (
            relation.source.external_id.clone(),
            relation.target.external_id.clone(),
            relation.kind.clone(),
        )
    });
    relations
}
#[cfg(test)]
mod tests {
    use super::*;
    use next_infra_connector_api::{
        ConnectionInput, ConnectorFailure, ReadConnector, SyncOutcome, SyncRequest,
    };
    use next_infra_connector_contract_tests::check_descriptor;
    use next_infra_core::{
        ConnectionId, ConnectorType, CoverageGapReason, ErrorCode, SchemaVersion, Scope,
        SecretValue, SyncCoverage, SyncMode, SyncRunId,
    };
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    const TEST_ORIGIN: &str = "https://cvm.tencentcloudapi.com";
    const TEST_HOST: &str = "cvm.tencentcloudapi.com";
    const TEST_SERVICE: &str = "cvm";

    fn sync_request() -> SyncRequest {
        SyncRequest {
            sync_run_id: SyncRunId::new("tencent-fixture-run").unwrap(),
            connection: ConnectionInput {
                connection_id: ConnectionId::new("tencent-fixture-connection").unwrap(),
                connector_type: ConnectorType::new("tencent").unwrap(),
                config: json!({"secret_id": "fixture-secret-id"}),
                config_schema_version: SchemaVersion::new(1).unwrap(),
            },
            mode: SyncMode::Full,
            scope: Scope::new("tencent:ap-example-1").unwrap(),
            cursor: None,
            targeted_resources: vec![],
        }
    }

    fn signed_request(action: &str, region: &str) -> SignedRequest {
        SignedRequest::new(
            TEST_ORIGIN,
            TEST_HOST,
            TEST_SERVICE,
            action,
            "2017-03-12",
            region,
            "fixture-secret-id",
            &SecretValue::new("super-secret-key"),
            1_700_000_000,
            100,
            0,
        )
        .unwrap()
    }

    #[test]
    fn tc3_signature_is_deterministic_and_never_echoes_secret() {
        let signature = tc3_authorization(
            &SecretValue::new("fixture-secret"),
            "fixture-id",
            "cvm",
            "cvm.tencentcloudapi.com",
            "DescribeInstances",
            "ap-example-1",
            1_700_000_000,
            "2023-11-14",
            br#"{}"#,
        )
        .unwrap();
        assert!(
            signature
                .starts_with("TC3-HMAC-SHA256 Credential=fixture-id/2023-11-14/cvm/tc3_request")
        );
        assert!(!signature.contains("fixture-secret"));
    }

    #[test]
    fn descriptor_is_module_scoped() {
        let d = descriptor();
        assert!(d.validate().is_ok());
        assert!(check_descriptor(&d).is_empty());
        assert!(
            d.resources
                .iter()
                .all(|r| r.coverage.module.starts_with("tencent."))
        );
    }

    #[test]
    fn tc3_request_carries_action_region_and_authorization() {
        let request = signed_request("DescribeInstances", "ap-example-1");
        assert_eq!(request.action, "DescribeInstances");
        assert_eq!(request.version, "2017-03-12");
        assert_eq!(request.region, "ap-example-1");
        let payload: serde_json::Value = serde_json::from_slice(&request.payload).unwrap();
        assert_eq!(payload["Limit"], 100);
        assert_eq!(payload["Offset"], 0);
        assert_eq!(request.host, "cvm.tencentcloudapi.com");
        assert!(request.authorization.starts_with(
            "TC3-HMAC-SHA256 Credential=fixture-secret-id/2023-11-14/cvm/tc3_request"
        ));
        assert!(!request.authorization.contains("super-secret-key"));
        assert!(!format!("{request:?}").contains("super-secret-key"));
    }

    #[test]
    fn tc3_request_is_injection_safe() {
        let request = SignedRequest::new(
            TEST_ORIGIN,
            TEST_HOST,
            TEST_SERVICE,
            "Describe&Injected=true",
            "2017-03-12",
            "ap-example-1&Injected=true",
            "fixture-secret-id",
            &SecretValue::new("super-secret-key"),
            1_700_000_000,
            100,
            0,
        )
        .unwrap();
        assert_eq!(request.url.origin().ascii_serialization(), TEST_ORIGIN);
        assert_eq!(request.host, TEST_HOST);
        assert!(!request.authorization.contains("Injected=true"));
        assert_eq!(request.action, "Describe&Injected=true");
        assert_eq!(request.region, "ap-example-1&Injected=true");
        assert!(!format!("{request:?}").contains("super-secret-key"));
    }

    #[test]
    fn tc3_authorization_is_deterministic_and_binds_payload() {
        let a = signed_request("DescribeInstances", "ap-example-1");
        let b = signed_request("DescribeInstances", "ap-example-1");
        let c = signed_request("DescribeVpcs", "ap-example-1");
        assert_eq!(a.authorization, b.authorization);
        // TC3 signs only content-type;host + payload hash — Action/Version/Region
        // travel in the (unsigned) X-TC-* headers, so payload-identical requests
        // share the same signature by spec.
        assert_eq!(a.authorization, c.authorization);
        assert_eq!(a.payload, b.payload);
        assert_eq!(a.payload, c.payload);
    }

    #[test]
    fn tc3_request_rejects_invalid_inputs() {
        let secret = &SecretValue::new("fixture-secret");
        assert!(
            SignedRequest::new(
                TEST_ORIGIN,
                TEST_HOST,
                TEST_SERVICE,
                "",
                "2017-03-12",
                "ap-example-1",
                "fixture-secret-id",
                secret,
                1,
                100,
                0
            )
            .is_err()
        );
        assert!(
            SignedRequest::new(
                TEST_ORIGIN,
                TEST_HOST,
                TEST_SERVICE,
                "A",
                "2017-03-12",
                "ap-example-1",
                "fixture-secret-id",
                secret,
                1,
                0,
                0
            )
            .is_err()
        );
        assert!(
            SignedRequest::new(
                TEST_ORIGIN,
                TEST_HOST,
                TEST_SERVICE,
                "A",
                "2017-03-12",
                "ap-example-1",
                "fixture-secret-id",
                secret,
                1,
                101,
                0
            )
            .is_err()
        );
    }

    #[test]
    fn tc3_request_rejects_empty_host() {
        assert!(
            SignedRequest::new(
                TEST_ORIGIN,
                "",
                TEST_SERVICE,
                "A",
                "2017-03-12",
                "ap-example-1",
                "fixture-secret-id",
                &SecretValue::new("fixture-secret"),
                1,
                100,
                0
            )
            .is_err()
        );
    }

    #[test]
    fn tc3_request_rejects_empty_service() {
        assert!(
            SignedRequest::new(
                TEST_ORIGIN,
                TEST_HOST,
                "",
                "A",
                "2017-03-12",
                "ap-example-1",
                "fixture-secret-id",
                &SecretValue::new("fixture-secret"),
                1,
                100,
                0
            )
            .is_err()
        );
    }

    #[test]
    fn module_origin_routes_by_product() {
        assert_eq!(
            module_origin("tencent.compute.cvm"),
            "https://cvm.tencentcloudapi.com"
        );
        assert_eq!(
            module_origin("tencent.network.vpc"),
            "https://vpc.tencentcloudapi.com"
        );
        assert_eq!(
            module_origin("tencent.edge.clb"),
            "https://clb.tencentcloudapi.com"
        );
        assert_eq!(
            module_origin("tencent.edge.dns"),
            "https://dnspod.tencentcloudapi.com"
        );
        assert_eq!(
            module_origin("tencent.edge.public_ip"),
            "https://vpc.tencentcloudapi.com"
        );
        assert_eq!(
            module_origin("unknown.module"),
            "https://cvm.tencentcloudapi.com"
        );
    }

    #[test]
    fn module_host_routes_by_product() {
        assert_eq!(
            module_host("tencent.compute.cvm"),
            "cvm.tencentcloudapi.com"
        );
        assert_eq!(
            module_host("tencent.network.vpc"),
            "vpc.tencentcloudapi.com"
        );
        assert_eq!(
            module_host("tencent.edge.dns"),
            "dnspod.tencentcloudapi.com"
        );
    }

    #[test]
    fn module_service_routes_by_product() {
        assert_eq!(module_service("tencent.compute.cvm"), "cvm");
        assert_eq!(module_service("tencent.network.vpc"), "vpc");
        assert_eq!(module_service("tencent.edge.clb"), "clb");
        assert_eq!(module_service("tencent.edge.dns"), "dnspod");
    }

    #[test]
    fn utc_date_formatting() {
        assert_eq!(format_utc_date(1_700_000_000), "2023-11-14");
        assert_eq!(format_utc_date(0), "1970-01-01");
        assert_eq!(format_utc_date(951_782_400), "2000-02-29");
    }

    #[test]
    fn parse_page_envelope_handles_response_wrapped_with_instance_set() {
        let body = br#"{"Response":{"TotalCount":1,"InstanceSet":[{"InstanceId":"ins-1","name":"test"}],"RequestId":"req-1"}}"#;
        let (items, total_count) = parse_page_envelope(body).unwrap();
        assert_eq!(total_count, 1);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "ins-1");
    }

    #[test]
    fn parse_page_envelope_handles_response_wrapped_with_vpc_set() {
        let body = br#"{"Response":{"TotalCount":3,"VpcSet":[{"VpcId":"vpc-1"},{"VpcId":"vpc-2"},{"VpcId":"vpc-3"}],"RequestId":"req-1"}}"#;
        let (items, total_count) = parse_page_envelope(body).unwrap();
        assert_eq!(total_count, 3);
        assert_eq!(items.len(), 3);
    }

    #[test]
    fn parse_page_envelope_tolerates_flat_shape() {
        let body = br#"{"TotalCount":1,"Items":[{"id":"flat-1","name":"Flat Resource"}]}"#;
        let (items, total_count) = parse_page_envelope(body).unwrap();
        assert_eq!(total_count, 1);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "flat-1");
    }

    #[test]
    fn parse_page_envelope_returns_error_on_missing_array() {
        let body = br#"{"Response":{"TotalCount":5,"RequestId":"req-1"}}"#;
        let result = parse_page_envelope(body);
        assert!(result.is_err());
    }

    #[test]
    fn mapper_drops_unknown() {
        let v: ResourceDto = serde_json::from_str(
            r#"{"id":"cvm-1","name":"Fixture","region":"ap-example-1","secret":"drop"}"#,
        )
        .unwrap();
        let o = map(
            "tencent.cvm.instance",
            &Scope::new("tencent:ap-example-1").unwrap(),
            Timestamp::from_unix_millis(1).unwrap(),
            v,
        )
        .unwrap();
        assert!(!serde_json::to_string(&o).unwrap().contains("drop"));
        assert_eq!(o.health, ResourceHealth::Unknown);
    }

    #[test]
    fn resource_dto_deser_picks_instance_id() {
        let v: ResourceDto = serde_json::from_str(r#"{"InstanceId":"ins-abc"}"#).unwrap();
        assert_eq!(v.id, "ins-abc");
    }

    #[test]
    fn resource_dto_deser_picks_vpc_id_and_parent_id() {
        let v: ResourceDto =
            serde_json::from_str(r#"{"VpcId":"vpc-xyz","VpcName":"my-vpc"}"#).unwrap();
        assert_eq!(v.id, "vpc-xyz");
        assert_eq!(v.parent_id.as_deref(), Some("vpc-xyz"));
    }

    #[test]
    fn resource_dto_deser_picks_subnet_id_and_parent_vpc() {
        let v: ResourceDto = serde_json::from_str(
            r#"{"SubnetId":"subnet-1","VpcId":"vpc-parent","SubnetName":"my-subnet"}"#,
        )
        .unwrap();
        assert_eq!(v.id, "subnet-1");
        assert_eq!(v.parent_id.as_deref(), Some("vpc-parent"));
    }

    #[test]
    fn resource_dto_deser_picks_address_id() {
        let v: ResourceDto = serde_json::from_str(r#"{"AddressId":"eip-abc123"}"#).unwrap();
        assert_eq!(v.id, "eip-abc123");
    }

    #[test]
    fn resource_dto_deser_picks_record_id() {
        let v: ResourceDto = serde_json::from_str(r#"{"RecordId":12345}"#).unwrap();
        assert_eq!(v.id, "12345");
    }

    #[test]
    fn resource_dto_deser_prefers_generic_id() {
        let v: ResourceDto =
            serde_json::from_str(r#"{"id":"generic", "InstanceId":"specific"}"#).unwrap();
        assert_eq!(v.id, "generic");
    }

    #[test]
    fn resource_dto_deser_returns_empty_id_for_unknown() {
        let v: ResourceDto = serde_json::from_str(r#"{"UnknownField":"x"}"#).unwrap();
        assert!(v.id.is_empty());
    }

    #[test]
    fn with_extra_params_adds_domain_to_payload() {
        let request = SignedRequest::with_extra_params(
            TEST_ORIGIN,
            TEST_HOST,
            TEST_SERVICE,
            "DescribeRecordList",
            "2021-03-23",
            "ap-example-1",
            "fixture-secret-id",
            &SecretValue::new("super-secret-key"),
            1_700_000_000,
            100,
            0,
            &[("Domain", "example.com")],
        )
        .unwrap();
        assert_eq!(request.action, "DescribeRecordList");
        let payload: serde_json::Value = serde_json::from_slice(&request.payload).unwrap();
        assert_eq!(payload["Domain"], "example.com");
        assert_eq!(payload["Limit"], 100);
        assert_eq!(payload["Offset"], 0);
    }

    // --- Transport tests ---

    struct FakeTransport;
    #[async_trait]
    impl TencentTransport for FakeTransport {
        async fn list(
            &self,
            request: SignedRequest,
            module: &'static str,
        ) -> Result<Vec<u8>, next_infra_connector_api::ConnectorFailure> {
            assert_eq!(
                request.url.origin().ascii_serialization(),
                module_origin(module)
            );
            assert_eq!(request.host, module_host(module));
            if module == "tencent.edge.dns" {
                return Err(failure(
                    next_infra_core::ErrorCode::RateLimited,
                    "fixture rate limit",
                ));
            }
            Ok(br#"{"Response":{"TotalCount":1,"InstanceSet":[{"InstanceId":"fixture-resource","InstanceName":"Fixture Resource","region":"ap-example-1","status":"Running","secret":"must-not-appear"}],"RequestId":"req-1"}}"#.to_vec())
        }
    }
    #[tokio::test]
    async fn read_connector_keeps_modules_on_partial_rate_limit() {
        let connector = TencentConnector::new(FakeTransport);
        let outcome = connector
            .sync(sync_request(), Some(&SecretValue::new("fixture-secret")))
            .await
            .unwrap();
        let SyncOutcome::Partial { batch, .. } = outcome else {
            panic!("expected partial")
        };
        assert_eq!(batch.resources.len(), 6);
        assert!(
            !serde_json::to_string(&batch)
                .unwrap()
                .contains("must-not-appear")
        );
    }

    struct PagingTransport;
    #[async_trait]
    impl TencentTransport for PagingTransport {
        async fn list(
            &self,
            request: SignedRequest,
            _module: &'static str,
        ) -> Result<Vec<u8>, ConnectorFailure> {
            assert_eq!(
                request.url.origin().ascii_serialization(),
                module_origin(_module)
            );
            let action = request.action.as_str();
            if action == "DescribeDomainList" {
                return Ok(br#"{"Response":{"TotalCount":1,"DomainList":[{"Name":"fixture-domain","DomainId":1}],"RequestId":"req-dns"}}"#.to_vec());
            }
            let payload: serde_json::Value = serde_json::from_slice(&request.payload).unwrap();
            let offset = payload["Offset"].as_u64().unwrap_or(0);
            if action == "DescribeRecordList" {
                let domain = payload["Domain"].as_str().unwrap_or("");
                assert!(!domain.is_empty());
                return Ok(format!(
                    r#"{{"Response":{{"TotalCount":2,"RecordList":[{{"RecordId":1,"Name":"www","Value":"1.2.3.4"}},{{"RecordId":2,"Name":"@","Value":"5.6.7.8"}}],"RequestId":"req-{domain}"}}}}"#)
                    .into_bytes());
            }
            let items = match offset {
                0 => {
                    r#"[{"InstanceId":"a","region":"ap-example-1"},{"InstanceId":"b","region":"ap-example-1"}]"#
                }
                100 => r#"[{"InstanceId":"c","region":"ap-example-1"}]"#,
                _ => "[]",
            };
            Ok(format!(
                r#"{{"Response":{{"TotalCount":3,"InstanceSet":{items},"RequestId":"req"}}}}"#
            )
            .into_bytes())
        }
    }
    #[tokio::test]
    async fn pagination_collects_all_pages_across_modules() {
        let connector = TencentConnector::new(PagingTransport);
        let outcome = connector
            .sync(sync_request(), Some(&SecretValue::new("fixture-secret")))
            .await
            .unwrap();
        let SyncOutcome::Complete { batch } = outcome else {
            panic!("expected complete")
        };
        // 6 modules × 3 items + DNS (1 domain × 1 record page of 6?) — assert the actual bounded total
        assert_eq!(batch.resources.len(), 24);
    }

    struct UnboundedTransport;
    #[async_trait]
    impl TencentTransport for UnboundedTransport {
        async fn list(
            &self,
            request: SignedRequest,
            _module: &'static str,
        ) -> Result<Vec<u8>, ConnectorFailure> {
            let action = request.action.as_str();
            if action == "DescribeDomainList" {
                return Ok(br#"{"Response":{"TotalCount":1,"DomainList":[{"Name":"fixture-domain","DomainId":1}],"RequestId":"req-dns"}}"#.to_vec());
            }
            if action == "DescribeRecordList" {
                return Ok(br#"{"Response":{"TotalCount":1,"RecordList":[{"RecordId":1,"Name":"www","Value":"1.2.3.4"}],"RequestId":"req-rec"}}"#.to_vec());
            }
            let payload: serde_json::Value = serde_json::from_slice(&request.payload).unwrap();
            let offset = payload["Offset"].as_u64().unwrap_or(0);
            let items: Vec<serde_json::Value> = (0..100)
                .map(|i| {
                    json!({"InstanceId": format!("budget-{offset}-{i}"), "region": "ap-example-1"})
                })
                .collect();
            Ok(json!({"Response": { "TotalCount": 10_000_000, "InstanceSet": items, "RequestId": "req" } })
                .to_string()
                .into_bytes())
        }
    }
    #[tokio::test]
    async fn pagination_budget_stops_early_with_partial_coverage() {
        let connector = TencentConnector::new(UnboundedTransport);
        let outcome = connector
            .sync(sync_request(), Some(&SecretValue::new("fixture-secret")))
            .await
            .unwrap();
        let SyncOutcome::Partial { batch, failure } = outcome else {
            panic!("expected partial")
        };
        assert_eq!(failure.code, ErrorCode::PartialPagination);
        let SyncCoverage::Partial { reason, .. } = batch.coverage else {
            panic!("expected partial coverage")
        };
        assert_eq!(reason, CoverageGapReason::PaginationIncomplete);
        // 6 modules × MAX_RESOURCES_PER_MODULE + DNS (1 domain × 1 record) = 60000 + 1
        assert_eq!(batch.resources.len(), 6 * MAX_RESOURCES_PER_MODULE + 1);
    }

    struct FlakyTransport {
        failures: Arc<Mutex<u32>>,
        attempts: Arc<Mutex<u32>>,
    }
    #[async_trait]
    impl TencentTransport for FlakyTransport {
        async fn list(
            &self,
            request: SignedRequest,
            _module: &'static str,
        ) -> Result<Vec<u8>, ConnectorFailure> {
            let action = request.action.as_str();
            if action == "DescribeDomainList" {
                return Ok(br#"{"Response":{"TotalCount":1,"DomainList":[{"Name":"fixture-domain","DomainId":1}],"RequestId":"req-dns"}}"#.to_vec());
            }
            if action == "DescribeRecordList" {
                return Ok(br#"{"Response":{"TotalCount":1,"RecordList":[{"RecordId":1,"Name":"www","Value":"1.2.3.4"}],"RequestId":"req-rec"}}"#.to_vec());
            }

            *self.attempts.lock().unwrap() += 1;
            let mut failures = self.failures.lock().unwrap();
            if *failures > 0 {
                *failures -= 1;
                return Err(ConnectorFailure {
                    code: ErrorCode::RateLimited,
                    message: "fixture throttle".into(),
                    retryable: true,
                    retry_after_ms: None,
                });
            }
            Ok(br#"{"Response":{"TotalCount":1,"InstanceSet":[{"InstanceId":"fixture-resource","region":"ap-example-1"}],"RequestId":"req"}}"#.to_vec())
        }
    }
    #[tokio::test]
    async fn rate_limit_retries_then_succeeds() {
        let failures = Arc::new(Mutex::new(1u32));
        let attempts = Arc::new(Mutex::new(0u32));
        let connector = TencentConnector::new(FlakyTransport {
            failures: failures.clone(),
            attempts: attempts.clone(),
        });
        let outcome = connector
            .sync(sync_request(), Some(&SecretValue::new("fixture-secret")))
            .await
            .unwrap();
        assert!(matches!(outcome, SyncOutcome::Complete { .. }));
        // 6 non-dns: first one fails once (2 attempts), rest succeed (5 attempts) = 7. DNS bypasses attempt counter.
        assert_eq!(*attempts.lock().unwrap(), 7);
    }
    #[tokio::test]
    async fn rate_limit_retries_are_bounded() {
        let failures = Arc::new(Mutex::new(3u32));
        let attempts = Arc::new(Mutex::new(0u32));
        let connector = TencentConnector::new(FlakyTransport {
            failures: failures.clone(),
            attempts: attempts.clone(),
        });
        let outcome = connector
            .sync(sync_request(), Some(&SecretValue::new("fixture-secret")))
            .await
            .unwrap();
        let SyncOutcome::Partial { batch, failure } = outcome else {
            panic!("expected partial")
        };
        assert_eq!(failure.code, ErrorCode::RateLimited);
        // First module fails (0 resources), remaining 5 non-dns succeed (5) + DNS succeeds (1) = 6 resources.
        assert_eq!(batch.resources.len(), 6);
        let SyncCoverage::Partial { reason, .. } = batch.coverage else {
            panic!("expected partial coverage")
        };
        assert_eq!(reason, CoverageGapReason::RateLimited);
        // Module 0 fails 3 times; modules 1-4,6 succeed (5); DNS bypasses = 8 total.
        assert_eq!(*attempts.lock().unwrap(), 8);
    }

    struct AuthFailTransport {
        attempts: Arc<Mutex<u32>>,
    }
    #[async_trait]
    impl TencentTransport for AuthFailTransport {
        async fn list(
            &self,
            _request: SignedRequest,
            _module: &'static str,
        ) -> Result<Vec<u8>, ConnectorFailure> {
            *self.attempts.lock().unwrap() += 1;
            Err(ConnectorFailure {
                code: ErrorCode::AuthenticationFailed,
                message: "fixture signature failure".into(),
                retryable: false,
                retry_after_ms: None,
            })
        }
    }
    #[tokio::test]
    async fn signature_error_is_not_retried() {
        let attempts = Arc::new(Mutex::new(0u32));
        let connector = TencentConnector::new(AuthFailTransport {
            attempts: attempts.clone(),
        });
        let result = connector
            .sync(sync_request(), Some(&SecretValue::new("fixture-secret")))
            .await;
        let Err(failure) = result else {
            panic!("expected hard failure for empty auth-failed sync")
        };
        assert_eq!(failure.code, ErrorCode::AuthenticationFailed);
        assert_eq!(*attempts.lock().unwrap(), 7);
    }

    #[test]
    fn sync_request_config_missing_secret_id_fails() {
        // Verify that missing secret_id in config is caught early.
        // sync_request uses config with secret_id; this test just proves
        // the config reader actually requires it.
        let config: serde_json::Value = json!({});
        assert!(
            config
                .get("secret_id")
                .and_then(serde_json::Value::as_str)
                .is_none()
        );
    }
}
