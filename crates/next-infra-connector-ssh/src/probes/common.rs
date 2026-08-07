//! Bounded parsers and resource mappers for the common SSH host probes.
//!
//! The parsers deliberately keep provider output out of their errors and output
//! models.  Only the fields frozen by CON-G6-02 cross the connector boundary.

use crate::{HostAlias, HostIdentity};
use next_infra_connector_api::{
    ConnectorFailure, RelationObservation, ResourceLocator, ResourceObservation,
};
use next_infra_core::{
    ErrorCode, EvidenceKey, ExternalId, FieldPath, LabelKey, RelationKind, ResourceHealth,
    ResourceKind, SchemaVersion, Scope, Timestamp,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use std::fmt;

pub const MAX_IDENTITY_STDOUT_BYTES: usize = 64 * 1024;
pub const MAX_UPTIME_STDOUT_BYTES: usize = 16 * 1024;
pub const MAX_FILESYSTEM_STDOUT_BYTES: usize = 256 * 1024;
pub const MAX_PROCESS_STDOUT_BYTES: usize = 256 * 1024;
pub const MAX_FILESYSTEM_ENTRIES: usize = 128;
pub const MAX_PROCESS_ROWS: usize = 4_096;
pub const MAX_FILESYSTEM_FIELD_BYTES: usize = 256;

const IDENTITY_MODULE: &str = "ssh.host.identity";
const UPTIME_MODULE: &str = "ssh.host.uptime";
const FILESYSTEM_MODULE: &str = "ssh.host.filesystems";
const PROCESS_MODULE: &str = "ssh.host.process-summary";

/// The only platform values accepted from `uname -s`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HostPlatform {
    Darwin,
    Linux,
}

impl HostPlatform {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Darwin => "darwin",
            Self::Linux => "linux",
        }
    }
}

/// Parsed output of `host.identity.v1`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostIdentityObservation {
    pub platform: HostPlatform,
    pub architecture: String,
}

/// The intentionally coarse uptime buckets exposed by the connector.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UptimeBucket {
    Lt1h,
    H1ToD1,
    D1ToD7,
    D7ToD30,
    Ge30d,
    Unknown,
}

impl UptimeBucket {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lt1h => "lt_1h",
            Self::H1ToD1 => "1h_1d",
            Self::D1ToD7 => "1d_7d",
            Self::D7ToD30 => "7d_30d",
            Self::Ge30d => "ge_30d",
            Self::Unknown => "unknown",
        }
    }
}

/// One POSIX `df -Pk` row.  The field names intentionally use KiB to make the
/// `1024-blocks` contract explicit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilesystemEntry {
    pub filesystem: String,
    pub blocks_kib: u64,
    pub used_kib: u64,
    pub available_kib: u64,
    pub capacity_percent: u8,
    pub mount: String,
}

/// Bounded filesystem summary.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilesystemSummary {
    pub entries: Vec<FilesystemEntry>,
}

/// Bounded process state summary.  Commands and process names never leave the
/// parser.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessSummary {
    pub total: u64,
    pub states: BTreeMap<String, u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommonParseErrorKind {
    MissingOutput,
    InvalidUtf8,
    UnsafeOutput,
    OutputLimit,
    MissingHeader,
    Malformed,
    Overflow,
    RowLimit,
}

/// A parser failure contains only a fixed module name and reason.  It never
/// stores or formats provider output.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct CommonParseError {
    pub module: &'static str,
    pub kind: CommonParseErrorKind,
}

impl CommonParseError {
    pub const fn new(module: &'static str, kind: CommonParseErrorKind) -> Self {
        Self { module, kind }
    }

    pub const fn bounded(self) -> bool {
        matches!(
            self.kind,
            CommonParseErrorKind::OutputLimit | CommonParseErrorKind::RowLimit
        )
    }

    /// Convert a parser failure to the connector API without exposing any
    /// provider text.  Transport failures are classified by the transport;
    /// parser failures are always invalid responses.
    pub fn connector_failure(self) -> ConnectorFailure {
        ConnectorFailure {
            code: ErrorCode::InvalidResponse,
            message: "SSH common probe response is invalid".into(),
            retryable: false,
            retry_after_ms: None,
        }
    }
}

impl fmt::Display for CommonParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SSH common probe response is invalid")
    }
}

impl std::error::Error for CommonParseError {}

/// Parse the exact two-line identity response.
pub fn parse_identity(input: &[u8]) -> Result<HostIdentityObservation, CommonParseError> {
    let text = checked_text(input, MAX_IDENTITY_STDOUT_BYTES, IDENTITY_MODULE)?;
    let mut lines = text.split('\n').collect::<Vec<_>>();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    if lines.len() != 2 {
        return Err(error(IDENTITY_MODULE, CommonParseErrorKind::Malformed));
    }
    let platform_line = lines[0].trim_end_matches('\r');
    let architecture = lines[1].trim_end_matches('\r');
    let platform = match platform_line {
        "Darwin" => HostPlatform::Darwin,
        "Linux" => HostPlatform::Linux,
        _ => return Err(error(IDENTITY_MODULE, CommonParseErrorKind::Malformed)),
    };
    if architecture.is_empty()
        || architecture.len() > 64
        || !architecture
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
    {
        return Err(error(IDENTITY_MODULE, CommonParseErrorKind::Malformed));
    }
    Ok(HostIdentityObservation {
        platform,
        architecture: architecture.to_owned(),
    })
}

/// Parse only the uptime duration after the literal ` up ` segment.  Unknown
/// but otherwise safe duration formats map to `Unknown` rather than retaining
/// a provider-specific string.
pub fn parse_uptime(input: &[u8]) -> Result<UptimeBucket, CommonParseError> {
    let text = checked_text(input, MAX_UPTIME_STDOUT_BYTES, UPTIME_MODULE)?;
    let start = text
        .find(" up ")
        .ok_or_else(|| error(UPTIME_MODULE, CommonParseErrorKind::Malformed))?;
    let after_up = &text[start + 4..];
    let mut segments = after_up.split(',').map(str::trim);
    let first = segments
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| error(UPTIME_MODULE, CommonParseErrorKind::Malformed))?;

    let minutes = if let Some(days) = parse_day_count(first) {
        let mut minutes = days.checked_mul(24 * 60);
        if days == 0
            && let Some(clock) = segments.next().and_then(parse_clock_minutes)
        {
            minutes = Some(clock);
        }
        minutes
    } else if let Some(clock) = parse_clock_minutes(first) {
        Some(clock)
    } else {
        parse_unit_duration_minutes(first)
    };

    Ok(minutes
        .map(bucket_for_minutes)
        .unwrap_or(UptimeBucket::Unknown))
}

/// Parse a POSIX `df -Pk` response, enforcing the header and row/field caps.
pub fn parse_filesystems(input: &[u8]) -> Result<FilesystemSummary, CommonParseError> {
    let text = checked_text(input, MAX_FILESYSTEM_STDOUT_BYTES, FILESYSTEM_MODULE)?;
    let mut lines = text.lines();
    let header = lines
        .by_ref()
        .find(|line| !line.trim().is_empty())
        .ok_or_else(|| error(FILESYSTEM_MODULE, CommonParseErrorKind::MissingHeader))?;
    if !is_posix_df_header(header) {
        return Err(error(
            FILESYSTEM_MODULE,
            CommonParseErrorKind::MissingHeader,
        ));
    }

    let mut entries = Vec::new();
    for line in lines.filter(|line| !line.trim().is_empty()) {
        if entries.len() >= MAX_FILESYSTEM_ENTRIES {
            return Err(error(FILESYSTEM_MODULE, CommonParseErrorKind::RowLimit));
        }
        let mut fields = line.split_ascii_whitespace();
        let filesystem = fields
            .next()
            .ok_or_else(|| error(FILESYSTEM_MODULE, CommonParseErrorKind::Malformed))?;
        let blocks_kib = parse_u64(fields.next(), FILESYSTEM_MODULE)?;
        let used_kib = parse_u64(fields.next(), FILESYSTEM_MODULE)?;
        let available_kib = parse_u64(fields.next(), FILESYSTEM_MODULE)?;
        let capacity = fields
            .next()
            .ok_or_else(|| error(FILESYSTEM_MODULE, CommonParseErrorKind::Malformed))?;
        let mount = fields.collect::<Vec<_>>().join(" ");
        if mount.is_empty()
            || mount.len() > MAX_FILESYSTEM_FIELD_BYTES
            || filesystem.len() > MAX_FILESYSTEM_FIELD_BYTES
            || !safe_field(filesystem)
            || !safe_field(&mount)
        {
            return Err(error(FILESYSTEM_MODULE, CommonParseErrorKind::Malformed));
        }
        let capacity_percent = capacity
            .strip_suffix('%')
            .ok_or_else(|| error(FILESYSTEM_MODULE, CommonParseErrorKind::Malformed))?
            .parse::<u8>()
            .map_err(|_| error(FILESYSTEM_MODULE, CommonParseErrorKind::Overflow))?;
        if capacity_percent > 100 {
            return Err(error(FILESYSTEM_MODULE, CommonParseErrorKind::Malformed));
        }
        entries.push(FilesystemEntry {
            filesystem: filesystem.to_owned(),
            blocks_kib,
            used_kib,
            available_kib,
            capacity_percent,
            mount,
        });
    }
    entries.sort_by(|left, right| {
        (
            &left.mount,
            &left.filesystem,
            left.blocks_kib,
            left.used_kib,
            left.available_kib,
            left.capacity_percent,
        )
            .cmp(&(
                &right.mount,
                &right.filesystem,
                right.blocks_kib,
                right.used_kib,
                right.available_kib,
                right.capacity_percent,
            ))
    });
    Ok(FilesystemSummary { entries })
}

/// Parse `ps -Ao state=,comm=` while discarding the command/name column.
pub fn parse_process_summary(input: &[u8]) -> Result<ProcessSummary, CommonParseError> {
    let text = checked_text(input, MAX_PROCESS_STDOUT_BYTES, PROCESS_MODULE)?;
    let mut states = fixed_state_counts();
    let mut total = 0_u64;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        if total >= MAX_PROCESS_ROWS as u64 {
            return Err(error(PROCESS_MODULE, CommonParseErrorKind::RowLimit));
        }
        let state = line
            .split_ascii_whitespace()
            .next()
            .and_then(|value| value.as_bytes().first().copied())
            .ok_or_else(|| error(PROCESS_MODULE, CommonParseErrorKind::Malformed))?;
        if !state.is_ascii() {
            return Err(error(PROCESS_MODULE, CommonParseErrorKind::Malformed));
        }
        let bucket = match state {
            b'R' | b'r' => "running",
            b'S' | b's' => "sleeping",
            b'T' | b't' => "stopped",
            b'Z' | b'z' => "zombie",
            _ => "other",
        };
        *states.get_mut(bucket).expect("fixed process state bucket") += 1;
        total = total
            .checked_add(1)
            .ok_or_else(|| error(PROCESS_MODULE, CommonParseErrorKind::Overflow))?;
    }
    Ok(ProcessSummary { total, states })
}

/// Probe stdout supplied to the common mapper.  `None` denotes a transport or
/// scheduling failure and is recorded as a module failure without affecting
/// successful modules.
#[derive(Clone, Copy, Debug, Default)]
pub struct CommonProbeInput<'a> {
    pub identity: Option<&'a [u8]>,
    pub uptime: Option<&'a [u8]>,
    pub filesystems: Option<&'a [u8]>,
    pub process_summary: Option<&'a [u8]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommonModuleState {
    Complete,
    Partial,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CommonModuleResult {
    pub module: &'static str,
    pub collected: usize,
    pub bounded: bool,
    pub state: CommonModuleState,
    pub failure: Option<CommonParseError>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommonMapperOutput {
    pub resources: Vec<ResourceObservation>,
    pub relations: Vec<RelationObservation>,
    pub modules: Vec<CommonModuleResult>,
}

impl CommonMapperOutput {
    pub fn merge(mut self, mut other: Self) -> Self {
        self.resources.append(&mut other.resources);
        self.relations.append(&mut other.relations);
        self.modules.append(&mut other.modules);
        sort_output(&mut self);
        self
    }
}

/// Map the four common probes.  Identity is the endpoint contract: failure to
/// parse it is fatal, while uptime/filesystem/process failures remain partial.
pub fn map_common(
    host_identity: &HostIdentity,
    host_alias: &HostAlias,
    scope: &Scope,
    observed_at: Timestamp,
    probes: CommonProbeInput<'_>,
) -> Result<CommonMapperOutput, CommonParseError> {
    let identity_bytes = probes
        .identity
        .ok_or_else(|| error(IDENTITY_MODULE, CommonParseErrorKind::MissingOutput))?;
    let identity = parse_identity(identity_bytes)?;
    let mut modules = vec![CommonModuleResult {
        module: IDENTITY_MODULE,
        collected: 1,
        bounded: false,
        state: CommonModuleState::Complete,
        failure: None,
    }];

    let (uptime, uptime_failure) = match probes.uptime {
        Some(output) => match parse_uptime(output) {
            Ok(bucket) => (bucket, None),
            Err(failure) => (UptimeBucket::Unknown, Some(failure)),
        },
        None => (
            UptimeBucket::Unknown,
            Some(error(UPTIME_MODULE, CommonParseErrorKind::MissingOutput)),
        ),
    };
    let uptime_ok = uptime_failure.is_none();
    modules.push(module_result(
        UPTIME_MODULE,
        if uptime_ok { 1 } else { 0 },
        uptime_failure,
    ));

    let (filesystems, filesystem_failure) = match probes.filesystems {
        Some(output) => match parse_filesystems(output) {
            Ok(summary) => (Some(summary), None),
            Err(failure) => (None, Some(failure)),
        },
        None => (
            None,
            Some(error(
                FILESYSTEM_MODULE,
                CommonParseErrorKind::MissingOutput,
            )),
        ),
    };
    let filesystem_collected = filesystems
        .as_ref()
        .map(|summary| summary.entries.len())
        .unwrap_or(0);
    modules.push(module_result(
        FILESYSTEM_MODULE,
        filesystem_collected,
        filesystem_failure,
    ));

    let (processes, process_failure) = match probes.process_summary {
        Some(output) => match parse_process_summary(output) {
            Ok(summary) => (Some(summary), None),
            Err(failure) => (None, Some(failure)),
        },
        None => (
            None,
            Some(error(PROCESS_MODULE, CommonParseErrorKind::MissingOutput)),
        ),
    };
    let process_collected = processes
        .as_ref()
        .map(|summary| summary.total as usize)
        .unwrap_or(0);
    modules.push(module_result(
        PROCESS_MODULE,
        process_collected,
        process_failure,
    ));

    let host_external_id = host_identity.external_id();
    let host_kind = kind("ssh.host");
    let host = ResourceObservation {
        kind: host_kind.clone(),
        external_id: host_external_id.clone(),
        name: "ssh-host".into(),
        display_name: format!("SSH host ({})", host_alias.expose()),
        scope: scope.clone(),
        labels: labels(identity.platform, "host"),
        health: if uptime_ok {
            ResourceHealth::Healthy
        } else {
            ResourceHealth::Unknown
        },
        attributes: json!({
            "platform": identity.platform.as_str(),
            "architecture": identity.architecture,
            "uptime_bucket": uptime.as_str(),
        }),
        attribute_schema_version: schema_version(),
        observed_at,
    };

    let filesystem_kind = kind("ssh.filesystem");
    let process_kind = kind("ssh.process-summary");
    let mut resources = vec![host];
    let mut relations = Vec::new();
    if let Some(summary) = filesystems {
        let external_id = ExternalId::new(format!("ssh-filesystems:v1:{}", host_identity.as_str()))
            .expect("static filesystem external ID format");
        resources.push(ResourceObservation {
            kind: filesystem_kind.clone(),
            external_id: external_id.clone(),
            name: "ssh-filesystems".into(),
            display_name: format!("SSH filesystems ({})", host_alias.expose()),
            scope: scope.clone(),
            labels: labels(identity.platform, "filesystem"),
            health: ResourceHealth::Unknown,
            attributes: json!({
                "host_identity": host_identity.as_str(),
                "entries": summary.entries,
            }),
            attribute_schema_version: schema_version(),
            observed_at,
        });
        relations.push(relation(
            &host_kind,
            &host_external_id,
            &filesystem_kind,
            &external_id,
            format!("ssh-provider-filesystems:{}", host_identity.as_str()),
            observed_at,
        ));
    }
    if let Some(summary) = processes {
        let external_id =
            ExternalId::new(format!("ssh-process-summary:v1:{}", host_identity.as_str()))
                .expect("static process external ID format");
        resources.push(ResourceObservation {
            kind: process_kind.clone(),
            external_id: external_id.clone(),
            name: "ssh-process-summary".into(),
            display_name: format!("SSH process summary ({})", host_alias.expose()),
            scope: scope.clone(),
            labels: labels(identity.platform, "process-summary"),
            health: ResourceHealth::Unknown,
            attributes: json!({
                "host_identity": host_identity.as_str(),
                "total": summary.total,
                "states": summary.states,
            }),
            attribute_schema_version: schema_version(),
            observed_at,
        });
        relations.push(relation(
            &host_kind,
            &host_external_id,
            &process_kind,
            &external_id,
            format!("ssh-provider-process-summary:{}", host_identity.as_str()),
            observed_at,
        ));
    }

    let mut output = CommonMapperOutput {
        resources,
        relations,
        modules,
    };
    sort_output(&mut output);
    Ok(output)
}

fn checked_text<'a>(
    input: &'a [u8],
    max_bytes: usize,
    module: &'static str,
) -> Result<&'a str, CommonParseError> {
    if input.len() > max_bytes {
        return Err(error(module, CommonParseErrorKind::OutputLimit));
    }
    let text =
        std::str::from_utf8(input).map_err(|_| error(module, CommonParseErrorKind::InvalidUtf8))?;
    if text
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err(error(module, CommonParseErrorKind::UnsafeOutput));
    }
    let lower = text.to_ascii_lowercase();
    if [
        "bearer ",
        "authorization:",
        "cookie:",
        "token=",
        "password=",
        "passwd=",
        "secret=",
        "-----begin",
        "private key",
    ]
    .iter()
    .any(|sentinel| lower.contains(sentinel))
    {
        return Err(error(module, CommonParseErrorKind::UnsafeOutput));
    }
    Ok(text)
}

fn parse_u64(value: Option<&str>, module: &'static str) -> Result<u64, CommonParseError> {
    let value = value.ok_or_else(|| error(module, CommonParseErrorKind::Malformed))?;
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(error(module, CommonParseErrorKind::Malformed));
    }
    value
        .parse::<u64>()
        .map_err(|_| error(module, CommonParseErrorKind::Overflow))
}

fn safe_field(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_FILESYSTEM_FIELD_BYTES
        && !value.chars().any(char::is_control)
}

fn is_posix_df_header(header: &str) -> bool {
    let fields = header.split_ascii_whitespace().collect::<Vec<_>>();
    fields.len() >= 6
        && fields[0] == "Filesystem"
        && fields[1] == "1024-blocks"
        && fields[2] == "Used"
        && fields[3] == "Available"
        && fields[4] == "Capacity"
        && fields
            .windows(2)
            .any(|pair| pair[0] == "Mounted" && pair[1] == "on")
}

fn parse_day_count(value: &str) -> Option<u64> {
    let mut parts = value.split_ascii_whitespace();
    let count = parts.next()?.parse::<u64>().ok()?;
    let unit = parts.next()?.to_ascii_lowercase();
    if parts.next().is_some() {
        return None;
    }
    match unit.as_str() {
        "day" | "days" => Some(count),
        "week" | "weeks" => count.checked_mul(7),
        _ => None,
    }
}

fn parse_clock_minutes(value: &str) -> Option<u64> {
    let mut parts = value.split(':');
    let hours = parts.next()?.parse::<u64>().ok()?;
    let minutes = parts.next()?.parse::<u64>().ok()?;
    let seconds = parts.next().and_then(|value| value.parse::<u64>().ok());
    if parts.next().is_some() || minutes >= 60 || seconds.is_some_and(|value| value >= 60) {
        return None;
    }
    hours.checked_mul(60)?.checked_add(minutes)
}

fn parse_unit_duration_minutes(value: &str) -> Option<u64> {
    let mut parts = value.split_ascii_whitespace();
    let count = parts.next()?.parse::<u64>().ok()?;
    let unit = parts.next()?.to_ascii_lowercase();
    if parts.next().is_some() {
        return None;
    }
    match unit.as_str() {
        "minute" | "minutes" | "min" | "mins" => Some(count),
        "hour" | "hours" | "hr" | "hrs" => count.checked_mul(60),
        "second" | "seconds" | "sec" | "secs" => Some(count / 60),
        "day" | "days" => count.checked_mul(24 * 60),
        "week" | "weeks" => count.checked_mul(7 * 24 * 60),
        _ => None,
    }
}

fn bucket_for_minutes(minutes: u64) -> UptimeBucket {
    match minutes {
        0..=59 => UptimeBucket::Lt1h,
        60..=1_439 => UptimeBucket::H1ToD1,
        1_440..=10_079 => UptimeBucket::D1ToD7,
        10_080..=43_199 => UptimeBucket::D7ToD30,
        _ => UptimeBucket::Ge30d,
    }
}

fn fixed_state_counts() -> BTreeMap<String, u64> {
    ["running", "sleeping", "stopped", "zombie", "other"]
        .into_iter()
        .map(|state| (state.to_owned(), 0))
        .collect()
}

fn module_result(
    module: &'static str,
    collected: usize,
    failure: Option<CommonParseError>,
) -> CommonModuleResult {
    CommonModuleResult {
        module,
        collected,
        bounded: failure.is_some_and(CommonParseError::bounded),
        state: if failure.is_some() {
            CommonModuleState::Partial
        } else {
            CommonModuleState::Complete
        },
        failure,
    }
}

fn sort_output(output: &mut CommonMapperOutput) {
    output
        .resources
        .sort_by_key(|resource| (resource.kind.clone(), resource.external_id.clone()));
    output.relations.sort_by_key(|relation| {
        (
            relation.source.kind.clone(),
            relation.source.external_id.clone(),
            relation.target.kind.clone(),
            relation.target.external_id.clone(),
            relation.kind.clone(),
            relation.evidence_key.clone(),
        )
    });
    output.modules.sort_by_key(|module| module.module);
}

fn labels(platform: HostPlatform, resource_type: &str) -> BTreeMap<LabelKey, String> {
    let mut labels = BTreeMap::new();
    labels.insert(
        LabelKey::new("ssh.platform").expect("static SSH label key"),
        platform.as_str().into(),
    );
    labels.insert(
        LabelKey::new("ssh.resource_type").expect("static SSH label key"),
        resource_type.into(),
    );
    labels
}

fn kind(value: &str) -> ResourceKind {
    ResourceKind::new(value).expect("static SSH resource kind")
}

fn relation(
    source_kind: &ResourceKind,
    source_external_id: &ExternalId,
    target_kind: &ResourceKind,
    target_external_id: &ExternalId,
    evidence_key: String,
    observed_at: Timestamp,
) -> RelationObservation {
    RelationObservation {
        source: ResourceLocator {
            kind: source_kind.clone(),
            external_id: source_external_id.clone(),
        },
        target: ResourceLocator {
            kind: target_kind.clone(),
            external_id: target_external_id.clone(),
        },
        kind: RelationKind::new("ssh.contains").expect("static SSH relation kind"),
        evidence_key: EvidenceKey::new(evidence_key).expect("static SSH evidence key"),
        field_path: FieldPath::new("attributes.host_identity").expect("static SSH field path"),
        observed_at,
    }
}

fn schema_version() -> SchemaVersion {
    SchemaVersion::new(1).expect("static SSH schema version")
}

const fn error(module: &'static str, kind: CommonParseErrorKind) -> CommonParseError {
    CommonParseError::new(module, kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const IDENTITY: &str = "9f7fd5e6-3bc8-4daa-ae6b-9dfdffb54743";

    fn host() -> (HostIdentity, HostAlias, Scope, Timestamp) {
        (
            HostIdentity::parse(IDENTITY).unwrap(),
            HostAlias::parse("fixture-host").unwrap(),
            Scope::new("fixture-scope").unwrap(),
            Timestamp::from_unix_millis(1_000).unwrap(),
        )
    }

    fn probes<'a>(
        identity: &'a [u8],
        uptime: &'a [u8],
        filesystems: &'a [u8],
        process_summary: &'a [u8],
    ) -> CommonProbeInput<'a> {
        CommonProbeInput {
            identity: Some(identity),
            uptime: Some(uptime),
            filesystems: Some(filesystems),
            process_summary: Some(process_summary),
        }
    }

    #[test]
    fn identity_accepts_darwin_and_linux_but_keeps_exact_two_lines() {
        assert_eq!(
            parse_identity(b"Darwin\narm64\n").unwrap(),
            HostIdentityObservation {
                platform: HostPlatform::Darwin,
                architecture: "arm64".into(),
            }
        );
        assert_eq!(
            parse_identity(b"Linux\nx86_64").unwrap().platform,
            HostPlatform::Linux
        );
        assert!(parse_identity(b"Darwin\narm64\nextra").is_err());
        assert!(parse_identity(b"Windows\nx86_64").is_err());
        assert!(parse_identity(b"Linux\narm 64").is_err());
    }

    #[test]
    fn uptime_maps_boundaries_and_unknown_formats_without_retaining_raw_text() {
        for (value, expected) in [
            ("12:00 up 59 mins, 1 user", UptimeBucket::Lt1h),
            ("12:00 up 1:00, 1 user", UptimeBucket::H1ToD1),
            ("12:00 up 1 day,  1:00, 1 user", UptimeBucket::D1ToD7),
            ("12:00 up 7 days,  1:00, 1 user", UptimeBucket::D7ToD30),
            ("12:00 up 30 days,  1:00, 1 user", UptimeBucket::Ge30d),
            ("12:00 up 3 fortnights, 1 user", UptimeBucket::Unknown),
        ] {
            assert_eq!(parse_uptime(value.as_bytes()).unwrap(), expected);
        }
        assert!(parse_uptime(b"12:00 no duration").is_err());
    }

    #[test]
    fn df_requires_posix_header_sorts_rows_and_checks_numbers() {
        let output = parse_filesystems(
            b"Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/b 20 2 18 10% /z\n/dev/a 10 1 9 10% /a\n",
        )
        .unwrap();
        assert_eq!(output.entries[0].mount, "/a");
        assert_eq!(output.entries[1].filesystem, "/dev/b");
        assert!(parse_filesystems(b"Filesystem Used\n/dev/a 1\n").is_err());
        assert!(parse_filesystems(
            b"Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/a 18446744073709551616 1 1 1% /a\n"
        )
        .is_err());
    }

    #[test]
    fn df_row_cap_and_sentinel_are_rejected_without_echoing_values() {
        let mut output =
            String::from("Filesystem 1024-blocks Used Available Capacity Mounted on\n");
        for index in 0..=MAX_FILESYSTEM_ENTRIES {
            output.push_str(&format!("/dev/{index} 1 1 0 100% /{index}\n"));
        }
        assert_eq!(
            parse_filesystems(output.as_bytes()).unwrap_err().kind,
            CommonParseErrorKind::RowLimit
        );
        let error = parse_filesystems(
            b"Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/a 1 1 0 0% /Bearer fixture-secret\n",
        )
        .unwrap_err();
        assert!(!format!("{error:?}").contains("fixture-secret"));
    }

    #[test]
    fn processes_count_fixed_buckets_and_discard_commands() {
        let summary =
            parse_process_summary(b" R fixture-command\nS launchd\nT stopped\nZ zombie\nD disk\n")
                .unwrap();
        assert_eq!(summary.total, 5);
        assert_eq!(summary.states["running"], 1);
        assert_eq!(summary.states["sleeping"], 1);
        assert_eq!(summary.states["stopped"], 1);
        assert_eq!(summary.states["zombie"], 1);
        assert_eq!(summary.states["other"], 1);
        let serialized = serde_json::to_string(&summary).unwrap();
        assert!(!serialized.contains("fixture-command"));
    }

    #[test]
    fn process_row_cap_and_control_or_sentinel_are_rejected() {
        let output = (0..=MAX_PROCESS_ROWS)
            .map(|_| "R process")
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            parse_process_summary(output.as_bytes()).unwrap_err().kind,
            CommonParseErrorKind::RowLimit
        );
        assert!(
            parse_process_summary(b"R Bearer fixture-secret\n")
                .unwrap_err()
                .kind
                == CommonParseErrorKind::UnsafeOutput
        );
        assert!(parse_process_summary(b"R\x01process\n").is_err());
    }

    #[test]
    fn map_common_has_stable_ids_relations_labels_and_partial_modules() {
        let (identity, alias, scope, observed_at) = host();
        let output = map_common(
            &identity,
            &alias,
            &scope,
            observed_at,
            probes(
                b"Linux\nx86_64\n",
                b"12:00 up 2 days, 1 user",
                b"Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/b 20 2 18 10% /z\n/dev/a 10 1 9 10% /a\n",
                b"R process-a\nS process-b\n",
            ),
        )
        .unwrap();
        assert_eq!(output.resources.len(), 3);
        assert_eq!(output.relations.len(), 2);
        assert_eq!(
            output.resources[0].external_id.as_str(),
            "ssh-filesystems:v1:9f7fd5e6-3bc8-4daa-ae6b-9dfdffb54743"
        );
        assert!(
            output
                .relations
                .iter()
                .all(|relation| relation.field_path.as_str() == "attributes.host_identity")
        );
        assert_eq!(
            output.resources[0].labels[&LabelKey::new("ssh.platform").unwrap()],
            "linux"
        );
        assert_eq!(
            output.resources[0].health,
            ResourceHealth::Unknown,
            "only the host health is meaningful; sorted first resource is filesystem"
        );
        let serialized = serde_json::to_string(&output.resources).unwrap();
        assert!(!serialized.contains("process-a"));
        assert!(!serialized.contains("process-b"));
    }

    #[test]
    fn map_common_preserves_successful_modules_when_one_parser_fails() {
        let (identity, alias, scope, observed_at) = host();
        let output = map_common(
            &identity,
            &alias,
            &scope,
            observed_at,
            CommonProbeInput {
                identity: Some(b"Darwin\narm64\n"),
                uptime: Some(b"12:00 up 2 hours, 1 user"),
                filesystems: Some(b"not a df response"),
                process_summary: Some(b"R process"),
            },
        )
        .unwrap();
        assert_eq!(output.resources.len(), 2);
        let filesystem = output
            .modules
            .iter()
            .find(|module| module.module == FILESYSTEM_MODULE)
            .unwrap();
        assert_eq!(filesystem.state, CommonModuleState::Partial);
        assert!(filesystem.failure.is_some());
        assert!(
            output
                .resources
                .iter()
                .any(|resource| resource.kind.as_str() == "ssh.process-summary")
        );
    }

    #[test]
    fn safe_unknown_uptime_is_still_a_successful_probe() {
        let (identity, alias, scope, observed_at) = host();
        let output = map_common(
            &identity,
            &alias,
            &scope,
            observed_at,
            probes(
                b"Linux\nx86_64\n",
                b"12:00 up 3 fortnights, 1 user",
                b"Filesystem 1024-blocks Used Available Capacity Mounted on\n",
                b"R process\n",
            ),
        )
        .unwrap();
        let host = output
            .resources
            .iter()
            .find(|resource| resource.kind.as_str() == "ssh.host")
            .unwrap();
        assert_eq!(host.attributes["uptime_bucket"], "unknown");
        assert_eq!(host.health, ResourceHealth::Healthy);
    }

    #[test]
    fn identity_failure_is_fatal_and_errors_are_redacted() {
        let (identity, alias, scope, observed_at) = host();
        let error = map_common(
            &identity,
            &alias,
            &scope,
            observed_at,
            CommonProbeInput {
                identity: Some(b"Linux\nBearer fixture-secret\n"),
                ..CommonProbeInput::default()
            },
        )
        .unwrap_err();
        assert_eq!(error.kind, CommonParseErrorKind::UnsafeOutput);
        assert!(!format!("{error:?}").contains("fixture-secret"));
        assert_eq!(error.connector_failure().code, ErrorCode::InvalidResponse);
    }

    #[test]
    fn attributes_drop_commands_and_unknown_fields() {
        let (identity, alias, scope, observed_at) = host();
        let output = map_common(
            &identity,
            &alias,
            &scope,
            observed_at,
            probes(
                b"Linux\nx86_64\n",
                b"12:00 up 1 hour, 1 user, load averages: 1 2 3",
                b"Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/a 10 1 9 10% /a\n",
                b"R secret-command-name\n",
            ),
        )
        .unwrap();
        let values = output
            .resources
            .iter()
            .map(|resource| resource.attributes.clone())
            .collect::<Vec<Value>>();
        assert!(values.iter().all(|value| {
            let serialized = value.to_string();
            !serialized.contains("load averages") && !serialized.contains("secret-command-name")
        }));
    }
}
