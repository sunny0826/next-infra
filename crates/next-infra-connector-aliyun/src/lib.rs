//! Bounded Aliyun read contract: ECS, VPC/network, and edge summaries.
use async_trait::async_trait;
use hmac::{Hmac, Mac};
use next_infra_connector_api::ResourceObservation;
use next_infra_core::{
    ConnectorCoverage, ConnectorCoverageLevel, ConnectorType, ExternalId, LabelKey, ResourceHealth,
    ResourceKind, SchemaVersion, Scope, SyncMode, Timestamp,
};
use serde::Deserialize;
use serde_json::json;
use sha1_hmac::Sha1 as Sha1Digest;
use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[async_trait]
pub trait AliyunTransport: Send + Sync {
    async fn list(
        &self,
        request: SignedRequest,
        module: &'static str,
    ) -> Result<Vec<u8>, next_infra_connector_api::ConnectorFailure>;
}
pub struct AliyunConnector<T> {
    descriptor: next_infra_connector_api::ConnectorDescriptor,
    transport: T,
}
impl<T> AliyunConnector<T> {
    pub fn new(transport: T) -> Self {
        Self {
            descriptor: descriptor(),
            transport,
        }
    }
}
#[async_trait]
impl<T: AliyunTransport> next_infra_connector_api::ReadConnector for AliyunConnector<T> {
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
                "Aliyun connector type mismatch",
            ));
        }
        if secret.is_none() {
            errors.push(issue(
                next_infra_core::ErrorCode::CredentialUnavailable,
                "Aliyun credential is unavailable",
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
                "Aliyun connector type mismatch",
            ));
        }
        let secret = secret.ok_or_else(|| {
            failure(
                next_infra_core::ErrorCode::CredentialUnavailable,
                "Aliyun credential is unavailable",
            )
        })?;
        let modules = [
            (
                "aliyun.compute.ecs",
                "aliyun.ecs.instance",
                "DescribeInstances",
                "2014-05-26",
            ),
            (
                "aliyun.network.vpc",
                "aliyun.vpc.vpc",
                "DescribeVpcs",
                "2016-04-28",
            ),
            (
                "aliyun.network.vswitch",
                "aliyun.vpc.vswitch",
                "DescribeVSwitches",
                "2016-04-28",
            ),
            (
                "aliyun.network.security_group",
                "aliyun.vpc.security_group",
                "DescribeSecurityGroups",
                "2014-05-26",
            ),
            (
                "aliyun.edge.slb",
                "aliyun.slb.load_balancer",
                "DescribeLoadBalancers",
                "2014-05-15",
            ),
            (
                "aliyun.edge.dns",
                "aliyun.dns.record",
                "DescribeDomainRecords",
                "2015-01-09",
            ),
            (
                "aliyun.edge.public_ip",
                "aliyun.ecs.public_ip",
                "DescribeEipAddresses",
                "2014-05-26",
            ),
        ];
        let access_key = request
            .connection
            .config
            .get("access_key_id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| next_infra_connector_api::ConnectorFailure {
                code: next_infra_core::ErrorCode::InvalidDomainValue,
                message: "Aliyun connection config requires access_key_id".into(),
                retryable: false,
                retry_after_ms: None,
            })?
            .to_owned();
        let region = request
            .scope
            .as_str()
            .strip_prefix("aliyun:")
            .unwrap_or(request.scope.as_str())
            .to_owned();
        let mut resources = vec![];
        let mut error = None;
        for (module, kind, action, version) in modules {
            let fetch = if module == "aliyun.edge.dns" {
                fetch_dns(
                    &self.transport,
                    &request.scope,
                    &region,
                    &access_key,
                    secret,
                )
                .await
            } else {
                fetch_module(
                    &self.transport,
                    module,
                    kind,
                    action,
                    version,
                    &request.scope,
                    &region,
                    &access_key,
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
        if resources.is_empty() && partial {
            return Err(error.unwrap());
        }
        let relations = provider_relations(&resources);
        let batch = next_infra_connector_api::ObservationBatch {
            resources,
            relations,
            coverage: coverage.clone(),
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
                "Aliyun outcome is invalid",
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

pub const ALIYUN_API_ORIGIN: &str = "https://ecs.aliyuncs.com";

/// Per-product RPC endpoint (Aliyun docs: each product has its own origin).
/// VPC RPC rejects PageSize above its accepted window; keep it modest.
pub fn module_page_size(module: &str) -> u32 {
    match module {
        "aliyun.network.vpc" | "aliyun.network.vswitch" | "aliyun.network.security_group" => 50,
        _ => PAGE_SIZE,
    }
}

pub fn module_origin(module: &str) -> &'static str {
    match module {
        "aliyun.compute.ecs" | "aliyun.ecs.public_ip" => "https://ecs.aliyuncs.com",
        "aliyun.network.vpc" | "aliyun.network.vswitch" => "https://vpc.aliyuncs.com",
        "aliyun.network.security_group" => ALIYUN_API_ORIGIN,
        "aliyun.edge.slb" => "https://slb.aliyuncs.com",
        "aliyun.edge.dns" => "https://dns.aliyuncs.com",
        _ => ALIYUN_API_ORIGIN,
    }
}
const PAGE_SIZE: u32 = 100;
const MAX_PAGES_PER_MODULE: u32 = 100;
const MAX_RESOURCES_PER_MODULE: usize = 10_000;
const MAX_RETRIES: u32 = 2;
const MAX_RETRY_AFTER_MS: u64 = 5_000;
type HmacSha1 = Hmac<Sha1Digest>;

pub struct SignedRequest {
    pub url: url::Url,
    pub access_key: String,
}
impl SignedRequest {
    #[allow(clippy::too_many_arguments)] // RPC signing binds all independent request fields.
    pub fn new(
        origin: &str,
        action: &str,
        version: &str,
        region: &str,
        access_key: &str,
        secret_key: &next_infra_core::SecretValue,
        nonce: &str,
        now_unix_secs: u64,
        page_number: u32,
        page_size: u32,
    ) -> Result<Self, String> {
        Self::build(
            origin,
            action,
            version,
            region,
            access_key,
            secret_key,
            nonce,
            now_unix_secs,
            page_number,
            page_size,
            &[],
        )
    }

    /// Same as [`Self::new`] but appends action-specific query parameters
    /// (e.g. `DomainName` for DNS record listing).
    #[allow(clippy::too_many_arguments)] // RPC signing binds all independent request fields.
    pub fn with_extra_params(
        origin: &str,
        action: &str,
        version: &str,
        region: &str,
        access_key: &str,
        secret_key: &next_infra_core::SecretValue,
        nonce: &str,
        now_unix_secs: u64,
        page_number: u32,
        page_size: u32,
        extra: &[(&str, &str)],
    ) -> Result<Self, String> {
        Self::build(
            origin,
            action,
            version,
            region,
            access_key,
            secret_key,
            nonce,
            now_unix_secs,
            page_number,
            page_size,
            extra,
        )
    }

    #[allow(clippy::too_many_arguments)] // RPC signing binds all independent request fields.
    fn build(
        origin: &str,
        action: &str,
        version: &str,
        region: &str,
        access_key: &str,
        secret_key: &next_infra_core::SecretValue,
        nonce: &str,
        now_unix_secs: u64,
        page_number: u32,
        page_size: u32,
        extra: &[(&str, &str)],
    ) -> Result<Self, String> {
        if action.is_empty()
            || version.is_empty()
            || region.is_empty()
            || access_key.is_empty()
            || nonce.is_empty()
        {
            return Err("invalid Aliyun request scope".into());
        }
        if page_number == 0 || page_size == 0 || page_size > 100 {
            return Err("invalid Aliyun pagination window".into());
        }
        let mut params = BTreeMap::new();
        params.insert("AccessKeyId".into(), access_key.to_string());
        params.insert("Action".into(), action.to_string());
        params.insert("Format".into(), "JSON".into());
        params.insert("PageNumber".into(), page_number.to_string());
        params.insert("PageSize".into(), page_size.to_string());
        params.insert("RegionId".into(), region.to_string());
        params.insert("SignatureMethod".into(), "HMAC-SHA1".into());
        params.insert("SignatureNonce".into(), nonce.to_string());
        params.insert("SignatureVersion".into(), "1.0".into());
        params.insert("Timestamp".into(), format_utc_iso8601(now_unix_secs));
        params.insert("Version".into(), version.to_string());
        for (key, value) in extra {
            params.insert((*key).to_string(), (*value).to_string());
        }
        let signature = sign_rpc_query(secret_key.expose(), &params)?;
        params.insert("Signature".into(), signature);
        let url = url::Url::parse(&format!("{origin}/?{}", canonical_query(&params)))
            .map_err(|_| "invalid Aliyun request URL".to_string())?;
        Ok(Self {
            url,
            access_key: access_key.into(),
        })
    }
}
impl std::fmt::Debug for SignedRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignedRequest")
            .field("url", &self.url)
            .field("access_key", &"[REDACTED]")
            .finish()
    }
}

fn canonical_query(params: &BTreeMap<String, String>) -> String {
    params
        .iter()
        .map(|(key, value)| format!("{}={}", rfc3986_encode(key), rfc3986_encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

fn sign_rpc_query(secret_key: &[u8], params: &BTreeMap<String, String>) -> Result<String, String> {
    let canonical = canonical_query(params);
    let string_to_sign = format!("GET&%2F&{}", rfc3986_encode(&canonical));
    let mut key = secret_key.to_vec();
    key.push(b'&');
    let mut mac = HmacSha1::new_from_slice(&key).map_err(|_| "invalid Aliyun secret key")?;
    mac.update(string_to_sign.as_bytes());
    Ok(b64_encode(&mac.finalize().into_bytes()))
}

fn rfc3986_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(byte))
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
fn b64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(char::from(BASE64_ALPHABET[(n >> 18) as usize & 63]));
        out.push(char::from(BASE64_ALPHABET[(n >> 12) as usize & 63]));
        out.push(if chunk.len() > 1 {
            char::from(BASE64_ALPHABET[(n >> 6) as usize & 63])
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            char::from(BASE64_ALPHABET[n as usize & 63])
        } else {
            '='
        });
    }
    out
}

fn format_utc_iso8601(unix_secs: u64) -> String {
    let (year, month, day) = civil_from_unix_secs(unix_secs);
    let rem = unix_secs % 86_400;
    let hour = rem / 3_600;
    let minute = (rem % 3_600) / 60;
    let second = rem % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
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

#[derive(Deserialize)]
struct PageEnvelope {
    total_count: u64,
    page_number: u32,
    page_size: u32,
    items: Vec<ResourceDto>,
}

/// Aliyun RPC list responses wrap items per action
/// (Instances.Instance, Vpcs.Vpc, DomainRecords.Record, ...). Extract the
/// nested array generically instead of assuming a flat `Items` field.
fn parse_page_envelope(body: &[u8]) -> Result<PageEnvelope, String> {
    let value: serde_json::Value = serde_json::from_slice(body)
        .map_err(|_| "Aliyun response is not valid JSON".to_string())?;
    let total_count = value
        .get("TotalCount")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let page_number = value
        .get("PageNumber")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(1) as u32;
    let page_size = value
        .get("PageSize")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as u32;
    let items = value
        .get("Items")
        .filter(|value| value.is_array())
        .or_else(|| {
            value.as_object().and_then(|map| {
                map.values()
                    .find(|value| value.is_object())
                    .and_then(|wrapped| {
                        wrapped
                            .as_object()
                            .and_then(|inner| inner.values().find(|value| value.is_array()))
                    })
            })
        })
        .and_then(|array| serde_json::from_value(array.clone()).ok())
        .unwrap_or_default();
    Ok(PageEnvelope {
        total_count,
        page_number,
        page_size,
        items,
    })
}

struct ModuleFetch {
    resources: Vec<ResourceObservation>,
    failure: Option<next_infra_connector_api::ConnectorFailure>,
}

#[allow(clippy::too_many_arguments)] // Bounded per-module fetch binds scope, kind and signing inputs.
async fn fetch_module<T: AliyunTransport>(
    transport: &T,
    module: &'static str,
    kind: &str,
    action: &str,
    version: &str,
    scope: &Scope,
    region: &str,
    access_key: &str,
    secret_key: &next_infra_core::SecretValue,
) -> ModuleFetch {
    let mut resources = Vec::new();
    let mut fetch_failure = None;
    let mut page = 1u32;
    loop {
        if page > MAX_PAGES_PER_MODULE || resources.len() >= MAX_RESOURCES_PER_MODULE {
            fetch_failure = Some(failure(
                next_infra_core::ErrorCode::PartialPagination,
                "Aliyun pagination exceeded its bounded budget",
            ));
            break;
        }
        let result = retry_list(transport, module, || {
            SignedRequest::new(
                module_origin(module),
                action,
                version,
                region,
                access_key,
                secret_key,
                &nonce(),
                now_unix_secs(),
                page,
                module_page_size(module),
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
        let envelope = match parse_page_envelope(&body) {
            Ok(envelope) => envelope,
            Err(_) => {
                fetch_failure = Some(failure(
                    next_infra_core::ErrorCode::InvalidResponse,
                    "Aliyun module response is invalid",
                ));
                break;
            }
        };
        if envelope.page_size == 0 {
            fetch_failure = Some(failure(
                next_infra_core::ErrorCode::InvalidResponse,
                "Aliyun page size is invalid",
            ));
            break;
        }
        if envelope.page_number != page {
            fetch_failure = Some(failure(
                next_infra_core::ErrorCode::InvalidResponse,
                "Aliyun page number is invalid",
            ));
            break;
        }
        let observations = envelope
            .items
            .into_iter()
            .map(|value| map(kind, scope, Timestamp::from_unix_millis(0).unwrap(), value))
            .collect::<Result<Vec<_>, _>>();
        resources.extend(match observations {
            Ok(observations) => observations,
            Err(_) => {
                fetch_failure = Some(failure(
                    next_infra_core::ErrorCode::InvalidResponse,
                    "Aliyun resource response is invalid",
                ));
                break;
            }
        });
        if u64::try_from(resources.len()).unwrap_or(u64::MAX) >= envelope.total_count {
            break;
        }
        page += 1;
    }
    ModuleFetch {
        resources,
        failure: fetch_failure,
    }
}

const MAX_DNS_DOMAINS: usize = 20;

/// DNS records require a two-step walk: list domains, then per-domain records.
async fn fetch_dns<T: AliyunTransport>(
    transport: &T,
    scope: &Scope,
    region: &str,
    access_key: &str,
    secret_key: &next_infra_core::SecretValue,
) -> ModuleFetch {
    let origin = module_origin("aliyun.edge.dns");
    let version = "2015-01-09";
    let mut domains: Vec<String> = Vec::new();
    let mut page = 1u32;
    loop {
        if page > MAX_PAGES_PER_MODULE || domains.len() >= MAX_DNS_DOMAINS {
            break;
        }
        let body = match retry_list(transport, "aliyun.edge.dns", || {
            SignedRequest::with_extra_params(
                origin,
                "DescribeDomains",
                version,
                region,
                access_key,
                secret_key,
                &nonce(),
                now_unix_secs(),
                page,
                module_page_size("aliyun.edge.dns"),
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
                        "Aliyun DNS domains response is invalid",
                    )),
                };
            }
        };
        let total_count = value
            .get("TotalCount")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let found = value
            .as_object()
            .and_then(|map| map.values().find(|v| v.is_object()))
            .and_then(|wrapped| wrapped.as_object())
            .and_then(|inner| inner.values().find(|v| v.is_array()))
            .map(|array| {
                array
                    .as_array()
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|item| {
                                item.get("DomainName").and_then(serde_json::Value::as_str)
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
                "Aliyun DNS records exceeded its bounded budget",
            ));
            break;
        }
        let mut page = 1u32;
        loop {
            if page > MAX_PAGES_PER_MODULE || resources.len() >= MAX_RESOURCES_PER_MODULE {
                fetch_failure = Some(failure(
                    next_infra_core::ErrorCode::PartialPagination,
                    "Aliyun DNS records exceeded its bounded budget",
                ));
                break;
            }
            let body = match retry_list(transport, "aliyun.edge.dns", || {
                SignedRequest::with_extra_params(
                    origin,
                    "DescribeDomainRecords",
                    version,
                    region,
                    access_key,
                    secret_key,
                    &nonce(),
                    now_unix_secs(),
                    page,
                    module_page_size("aliyun.edge.dns"),
                    &[("DomainName", &domain)],
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
            let envelope = match parse_page_envelope(&body) {
                Ok(envelope) => envelope,
                Err(_) => {
                    fetch_failure = Some(failure(
                        next_infra_core::ErrorCode::InvalidResponse,
                        "Aliyun DNS records response is invalid",
                    ));
                    break;
                }
            };
            if envelope.page_size == 0 || envelope.page_number != page {
                fetch_failure = Some(failure(
                    next_infra_core::ErrorCode::InvalidResponse,
                    "Aliyun DNS pagination is invalid",
                ));
                break;
            }
            let empty_page = envelope.items.is_empty();
            for value in envelope.items {
                match map("aliyun.dns.record", scope, at, value) {
                    Ok(observation) => resources.push(observation),
                    Err(_) => {
                        fetch_failure = Some(failure(
                            next_infra_core::ErrorCode::InvalidResponse,
                            "Aliyun DNS record mapping failed",
                        ));
                        break;
                    }
                }
            }
            if u64::try_from(resources.len()).unwrap_or(u64::MAX) >= envelope.total_count
                || empty_page
            {
                break;
            }
            page += 1;
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
    T: AliyunTransport,
    F: Fn() -> Result<SignedRequest, String>,
{
    let mut attempts = 0u32;
    loop {
        let request = build().map_err(|_| {
            failure(
                next_infra_core::ErrorCode::InvalidDomainValue,
                "Aliyun request scope is invalid",
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

fn nonce() -> String {
    uuid::Uuid::new_v4().to_string()
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
            "aliyun.ecs.instance",
            "aliyun.compute.ecs",
            ConnectorCoverageLevel::Supported,
        ),
        (
            "aliyun.vpc.vpc",
            "aliyun.network.vpc",
            ConnectorCoverageLevel::Supported,
        ),
        (
            "aliyun.vpc.vswitch",
            "aliyun.network.vswitch",
            ConnectorCoverageLevel::Supported,
        ),
        (
            "aliyun.vpc.security_group",
            "aliyun.network.security_group",
            ConnectorCoverageLevel::Partial,
        ),
        (
            "aliyun.slb.load_balancer",
            "aliyun.edge.slb",
            ConnectorCoverageLevel::Partial,
        ),
        (
            "aliyun.dns.record",
            "aliyun.edge.dns",
            ConnectorCoverageLevel::Partial,
        ),
        (
            "aliyun.ecs.public_ip",
            "aliyun.edge.public_ip",
            ConnectorCoverageLevel::Partial,
        ),
    ];
    next_infra_connector_api::ConnectorDescriptor {
        connector_type: ConnectorType::new("aliyun").unwrap(),
        connector_version: "1.0.0".into(),
        config_schema_version: SchemaVersion::new(1).unwrap(),
        auth: next_infra_connector_api::AuthDescriptor {
            kind: next_infra_connector_api::AuthKind::ApiKey,
            minimum_permissions: vec![
                "ecs:Describe*".into(),
                "vpc:Describe*".into(),
                "slb:Describe*".into(),
                "alidns:Describe*".into(),
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
                "aliyun.vpc.vpc",
                "aliyun.vpc.vswitch",
                "aliyun.contains",
                "aliyun.network.vpc_vswitch",
            ),
            relation(
                "aliyun.vpc.vpc",
                "aliyun.vpc.security_group",
                "aliyun.contains",
                "aliyun.network.vpc_security_group",
            ),
            relation(
                "aliyun.ecs.instance",
                "aliyun.ecs.public_ip",
                "aliyun.assigned",
                "aliyun.edge.instance_public_ip",
            ),
        ],
        sensitive_field_policy: vec![
            "AccessKey and SecretKey are transient signed request inputs".into(),
            "raw security rules and response bodies are excluded".into(),
        ],
        rate_limit: next_infra_connector_api::RateLimitGuidance {
            default_max_concurrency: 2,
            requests_per_minute: None,
            respects_retry_after: true,
        },
        recommended_sync_interval_secs: 900,
        known_gaps: vec![
            "writes, RAM policy mutation and products outside the listed modules are unsupported"
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

#[derive(Clone, Debug)]
pub struct ResourceDto {
    pub id: String,
    pub name: Option<String>,
    pub region: Option<String>,
    pub status: Option<String>,
    pub parent_id: Option<String>,
}

/// Aliyun products use per-resource id fields (InstanceId, VpcId, ...) and
/// some responses also carry a generic `id`; pick the first present one so a
/// duplicate never aborts deserialization.
impl<'de> serde::Deserialize<'de> for ResourceDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let object = value
            .as_object()
            .ok_or_else(|| serde::de::Error::custom("Aliyun resource is not an object"))?;
        let id = [
            "id",
            "InstanceId",
            "VpcId",
            "VSwitchId",
            "SecurityGroupId",
            "LoadBalancerId",
            "RecordId",
            "AllocationId",
        ]
        .iter()
        .find_map(|key| object.get(*key).and_then(serde_json::Value::as_str))
        .unwrap_or("")
        .to_owned();
        let field = |key: &str| {
            object
                .get(key)
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        };
        let parent_id = field("parent_id").or_else(|| field("VpcId"));
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
        return Err("Aliyun resource id is invalid".into());
    }
    Ok(ResourceObservation {
        kind: ResourceKind::new(kind).map_err(|_| "invalid kind")?,
        external_id: ExternalId::new(format!("{kind}:{}", v.id)).map_err(|_| "invalid id")?,
        name: v.id.clone(),
        display_name: v.name.unwrap_or_else(|| v.id.clone()),
        scope: scope.clone(),
        labels: BTreeMap::from([(
            LabelKey::new("aliyun.region").unwrap(),
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
            "aliyun.vpc.vswitch" | "aliyun.vpc.security_group" => {
                ("aliyun.vpc.vpc", "aliyun.contains")
            }
            "aliyun.ecs.public_ip" => ("aliyun.ecs.instance", "aliyun.assigned"),
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
                "aliyun:{relation_kind}:{}",
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

    fn sync_request() -> SyncRequest {
        SyncRequest {
            sync_run_id: SyncRunId::new("aliyun-fixture-run").unwrap(),
            connection: ConnectionInput {
                connection_id: ConnectionId::new("aliyun-fixture-connection").unwrap(),
                connector_type: ConnectorType::new("aliyun").unwrap(),
                config: json!({"access_key_id": "fixture-access"}),
                config_schema_version: SchemaVersion::new(1).unwrap(),
            },
            mode: SyncMode::Full,
            scope: Scope::new("aliyun:cn-example-1").unwrap(),
            cursor: None,
            targeted_resources: vec![],
        }
    }

    fn signed_request(action: &str) -> SignedRequest {
        SignedRequest::new(
            ALIYUN_API_ORIGIN,
            action,
            "2014-05-26",
            "cn-example-1",
            "fixture-access",
            &SecretValue::new("fixture-secret"),
            "nonce-1",
            1_700_000_000,
            1,
            100,
        )
        .unwrap()
    }

    #[test]
    fn descriptor_is_module_scoped() {
        let d = descriptor();
        assert!(d.validate().is_ok());
        assert!(check_descriptor(&d).is_empty());
        assert!(
            d.resources
                .iter()
                .all(|r| r.coverage.module.starts_with("aliyun."))
        );
    }

    #[test]
    fn rfc3986_encoding_matches_spec() {
        assert_eq!(rfc3986_encode("A-Za-z0-9-_.~"), "A-Za-z0-9-_.~");
        assert_eq!(rfc3986_encode("a b&c=d"), "a%20b%26c%3Dd");
        assert_eq!(rfc3986_encode("你好"), "%E4%BD%A0%E5%A5%BD");
    }

    #[test]
    fn base64_encoder_matches_standard_vectors() {
        assert_eq!(b64_encode(b""), "");
        assert_eq!(b64_encode(b"f"), "Zg==");
        assert_eq!(b64_encode(b"fo"), "Zm8=");
        assert_eq!(b64_encode(b"foo"), "Zm9v");
        assert_eq!(b64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(b64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(b64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn utc_timestamp_formatting() {
        assert_eq!(format_utc_iso8601(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(format_utc_iso8601(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_utc_iso8601(951_782_400), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn rpc_signature_is_deterministic_and_binds_params() {
        let a = signed_request("DescribeInstances");
        let b = signed_request("DescribeInstances");
        let c = signed_request("DescribeVpcs");
        assert_eq!(a.url.query(), b.url.query());
        assert_ne!(a.url.query(), c.url.query());
        assert!(!a.url.as_str().contains("fixture-secret"));
    }

    #[test]
    fn rpc_signature_round_trips_against_signed_params() {
        let request = signed_request("DescribeInstances");
        let mut params: BTreeMap<String, String> = request
            .url
            .query_pairs()
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect();
        let signature = params.remove("Signature").unwrap();
        assert_eq!(
            sign_rpc_query(b"fixture-secret", &params).unwrap(),
            signature
        );
    }

    #[test]
    fn rpc_signature_uses_the_spec_string_to_sign_shape() {
        let mut params = BTreeMap::new();
        params.insert("Action".into(), "DescribeInstances".into());
        let canonical = canonical_query(&params);
        assert_eq!(canonical, "Action=DescribeInstances");
        let string_to_sign = format!("GET&%2F&{}", rfc3986_encode(&canonical));
        assert_eq!(string_to_sign, "GET&%2F&Action%3DDescribeInstances");
    }

    #[test]
    fn rpc_request_is_injection_safe_and_never_echoes_secret() {
        let request = SignedRequest::new(
            ALIYUN_API_ORIGIN,
            "Describe&Injected=true",
            "2014-05-26",
            "cn-example-1&Injected=true",
            "fixture-access",
            &SecretValue::new("fixture-secret&="),
            "nonce-1",
            1_700_000_000,
            1,
            100,
        )
        .unwrap();
        let text = request.url.as_str();
        assert!(!text.contains("fixture-secret"));
        assert!(!text.contains("Describe&Injected=true"));
        assert!(!text.contains("cn-example-1&Injected=true"));
        let query: BTreeMap<String, String> = request
            .url
            .query_pairs()
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect();
        assert_eq!(query.get("Action").unwrap(), "Describe&Injected=true");
        assert_eq!(query.get("RegionId").unwrap(), "cn-example-1&Injected=true");
        assert_eq!(query.get("SignatureMethod").unwrap(), "HMAC-SHA1");
        assert_eq!(query.get("Format").unwrap(), "JSON");
        assert_eq!(query.get("SignatureVersion").unwrap(), "1.0");
        assert_eq!(query.get("Timestamp").unwrap(), "2023-11-14T22:13:20Z");
        assert!(query.contains_key("Signature"));
        assert!(!format!("{request:?}").contains("fixture-secret"));
    }

    #[test]
    fn rpc_request_rejects_invalid_inputs() {
        let secret = &SecretValue::new("fixture-secret");
        assert!(
            SignedRequest::new(
                ALIYUN_API_ORIGIN,
                "",
                "2014-05-26",
                "cn-example-1",
                "fixture-access",
                secret,
                "n",
                1,
                1,
                100
            )
            .is_err()
        );
        assert!(
            SignedRequest::new(
                ALIYUN_API_ORIGIN,
                "A",
                "2014-05-26",
                "cn-example-1",
                "fixture-access",
                secret,
                "",
                1,
                1,
                100
            )
            .is_err()
        );
        assert!(
            SignedRequest::new(
                ALIYUN_API_ORIGIN,
                "A",
                "2014-05-26",
                "cn-example-1",
                "fixture-access",
                secret,
                "n",
                1,
                0,
                100
            )
            .is_err()
        );
        assert!(
            SignedRequest::new(
                ALIYUN_API_ORIGIN,
                "A",
                "2014-05-26",
                "cn-example-1",
                "fixture-access",
                secret,
                "n",
                1,
                1,
                101
            )
            .is_err()
        );
    }

    #[test]
    fn mapper_drops_unknown() {
        let v: ResourceDto = serde_json::from_str(
            r#"{"id":"i-1","name":"Fixture","region":"cn-example-1","secret":"drop"}"#,
        )
        .unwrap();
        let o = map(
            "aliyun.ecs.instance",
            &Scope::new("aliyun:cn-example-1").unwrap(),
            Timestamp::from_unix_millis(1).unwrap(),
            v,
        )
        .unwrap();
        assert!(!serde_json::to_string(&o).unwrap().contains("drop"));
        assert_eq!(o.health, ResourceHealth::Unknown);
    }

    struct FakeTransport;
    #[async_trait]
    impl AliyunTransport for FakeTransport {
        async fn list(
            &self,
            request: SignedRequest,
            module: &'static str,
        ) -> Result<Vec<u8>, next_infra_connector_api::ConnectorFailure> {
            assert_eq!(
                request.url.origin().ascii_serialization(),
                module_origin(module)
            );
            if module == "aliyun.edge.dns" {
                return Err(failure(
                    next_infra_core::ErrorCode::RateLimited,
                    "fixture rate limit",
                ));
            }
            Ok(br#"{"TotalCount":1,"PageNumber":1,"PageSize":100,"Items":[{"id":"fixture-resource","name":"Fixture Resource","region":"cn-example-1","status":"Running","secret":"must-not-appear"}]}"#.to_vec())
        }
    }
    #[tokio::test]
    async fn read_connector_keeps_modules_on_partial_rate_limit() {
        let connector = AliyunConnector::new(FakeTransport);
        let outcome = connector
            .sync(sync_request(), Some(&SecretValue::new("fixture-secret")))
            .await
            .unwrap();
        let SyncOutcome::Partial { batch, failure: _ } = outcome else {
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
    impl AliyunTransport for PagingTransport {
        async fn list(
            &self,
            request: SignedRequest,
            module: &'static str,
        ) -> Result<Vec<u8>, ConnectorFailure> {
            if request
                .url
                .query_pairs()
                .any(|(key, value)| key == "Action" && value == "DescribeDomains")
            {
                return Ok(br#"{"TotalCount":1,"PageNumber":1,"PageSize":10,"Domains":{"Domain":[{"DomainName":"fixture-domain"}]}}"#.to_vec());
            }

            assert_eq!(
                request.url.origin().ascii_serialization(),
                module_origin(module)
            );
            let page: u32 = request
                .url
                .query_pairs()
                .find(|(key, _)| key.as_ref() == "PageNumber")
                .and_then(|(_, value)| value.parse().ok())
                .unwrap_or(1);
            let items = match page {
                1 => r#"[{"id":"a","region":"cn-example-1"},{"id":"b","region":"cn-example-1"}]"#,
                2 => r#"[{"id":"c","region":"cn-example-1"}]"#,
                _ => "[]",
            };
            Ok(
                format!(r#"{{"TotalCount":3,"PageNumber":{page},"PageSize":2,"Items":{items}}}"#)
                    .into_bytes(),
            )
        }
    }
    #[tokio::test]
    async fn pagination_collects_all_pages_across_modules() {
        let connector = AliyunConnector::new(PagingTransport);
        let outcome = connector
            .sync(sync_request(), Some(&SecretValue::new("fixture-secret")))
            .await
            .unwrap();
        let SyncOutcome::Complete { batch } = outcome else {
            panic!("expected complete")
        };
        assert_eq!(batch.resources.len(), 7 * 3);
    }

    struct UnboundedTransport;
    #[async_trait]
    impl AliyunTransport for UnboundedTransport {
        async fn list(
            &self,
            request: SignedRequest,
            _module: &'static str,
        ) -> Result<Vec<u8>, ConnectorFailure> {
            if request
                .url
                .query_pairs()
                .any(|(key, value)| key == "Action" && value == "DescribeDomains")
            {
                return Ok(br#"{"TotalCount":1,"PageNumber":1,"PageSize":10,"Domains":{"Domain":[{"DomainName":"fixture-domain"}]}}"#.to_vec());
            }

            let page: u32 = request
                .url
                .query_pairs()
                .find(|(key, _)| key.as_ref() == "PageNumber")
                .and_then(|(_, value)| value.parse().ok())
                .unwrap_or(1);
            let items: Vec<serde_json::Value> = (0..100)
                .map(|i| json!({"id": format!("budget-{page}-{i}"), "region": "cn-example-1"}))
                .collect();
            Ok(json!({
                    "TotalCount": 10_000_000,
                    "PageNumber": page,
                    "PageSize": 100,
                    "Items": items,

            })
            .to_string()
            .into_bytes())
        }
    }
    #[tokio::test]
    async fn pagination_budget_stops_early_with_partial_coverage() {
        let connector = AliyunConnector::new(UnboundedTransport);
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
        assert_eq!(batch.resources.len(), 7 * MAX_RESOURCES_PER_MODULE);
    }

    struct FlakyTransport {
        failures: Arc<Mutex<u32>>,
        attempts: Arc<Mutex<u32>>,
    }
    #[async_trait]
    impl AliyunTransport for FlakyTransport {
        async fn list(
            &self,
            request: SignedRequest,
            _module: &'static str,
        ) -> Result<Vec<u8>, ConnectorFailure> {
            if request
                .url
                .query_pairs()
                .any(|(key, value)| key == "Action" && value == "DescribeDomains")
            {
                return Ok(br#"{"TotalCount":1,"PageNumber":1,"PageSize":10,"Domains":{"Domain":[{"DomainName":"fixture-domain"}]}}"#.to_vec());
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
            Ok(br#"{"TotalCount":1,"PageNumber":1,"PageSize":100,"Items":[{"id":"fixture-resource","region":"cn-example-1"}]}"#.to_vec())
        }
    }
    #[tokio::test]
    async fn rate_limit_retries_then_succeeds() {
        let failures = Arc::new(Mutex::new(1u32));
        let attempts = Arc::new(Mutex::new(0u32));
        let connector = AliyunConnector::new(FlakyTransport {
            failures: failures.clone(),
            attempts: attempts.clone(),
        });
        let outcome = connector
            .sync(sync_request(), Some(&SecretValue::new("fixture-secret")))
            .await
            .unwrap();
        assert!(matches!(outcome, SyncOutcome::Complete { .. }));
        assert_eq!(*attempts.lock().unwrap(), 8);
    }
    #[tokio::test]
    async fn rate_limit_retries_are_bounded() {
        let failures = Arc::new(Mutex::new(3u32));
        let attempts = Arc::new(Mutex::new(0u32));
        let connector = AliyunConnector::new(FlakyTransport {
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
        assert_eq!(batch.resources.len(), 6);
        let SyncCoverage::Partial { reason, .. } = batch.coverage else {
            panic!("expected partial coverage")
        };
        assert_eq!(reason, CoverageGapReason::RateLimited);
        assert_eq!(*attempts.lock().unwrap(), 3 + 6);
    }

    struct AuthFailTransport {
        attempts: Arc<Mutex<u32>>,
    }
    #[async_trait]
    impl AliyunTransport for AuthFailTransport {
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
        let connector = AliyunConnector::new(AuthFailTransport {
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
}
