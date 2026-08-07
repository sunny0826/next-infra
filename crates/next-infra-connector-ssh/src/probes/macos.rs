//! Bounded parser and mapper for the fixed macOS launchd probe.

use crate::{HostIdentity, ServiceId};
use next_infra_connector_api::{
    ConnectorFailure, RelationObservation, ResourceLocator, ResourceObservation,
};
use next_infra_core::{
    ErrorCode, EvidenceKey, ExternalId, FieldPath, LabelKey, RelationKind, ResourceHealth,
    ResourceKind, SchemaVersion, Scope, Timestamp,
};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

pub const MAX_LAUNCHD_STDOUT_BYTES: usize = 512 * 1024;
pub const MAX_LAUNCHD_ROWS: usize = 2_048;
pub const MAX_LAUNCHD_OUTPUTS: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchdService {
    pub service_label: String,
    pub pid_present: bool,
    pub last_exit_status: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchdParseErrorKind {
    InvalidUtf8,
    UnsafeOutput,
    OutputLimit,
    MissingHeader,
    Malformed,
    Duplicate,
    RowLimit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LaunchdParseError {
    pub kind: LaunchdParseErrorKind,
}

impl LaunchdParseError {
    pub fn connector_failure(self) -> ConnectorFailure {
        ConnectorFailure {
            code: ErrorCode::InvalidResponse,
            message: "SSH launchd probe response is invalid".into(),
            retryable: false,
            retry_after_ms: None,
        }
    }
}

impl fmt::Display for LaunchdParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SSH launchd probe response is invalid")
    }
}

impl std::error::Error for LaunchdParseError {}

pub fn parse_launchd_services(
    input: &[u8],
    allowlist: &[ServiceId],
) -> Result<Vec<LaunchdService>, LaunchdParseError> {
    let text = checked_text(input)?;
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let header = lines
        .next()
        .ok_or_else(|| error(LaunchdParseErrorKind::MissingHeader))?;
    if header.split_ascii_whitespace().collect::<Vec<_>>() != ["PID", "Status", "Label"] {
        return Err(error(LaunchdParseErrorKind::MissingHeader));
    }

    let allowed = allowlist
        .iter()
        .map(ServiceId::expose)
        .collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    let mut services = Vec::new();
    for (row, line) in lines.enumerate() {
        if row >= MAX_LAUNCHD_ROWS {
            return Err(error(LaunchdParseErrorKind::RowLimit));
        }
        let fields = line.split_ascii_whitespace().collect::<Vec<_>>();
        let Some(label) = fields.last().copied() else {
            continue;
        };
        if !allowed.contains(label) {
            continue;
        }
        if fields.len() != 3 || ServiceId::parse(label).is_err() {
            return Err(error(LaunchdParseErrorKind::Malformed));
        }
        if !seen.insert(label) {
            return Err(error(LaunchdParseErrorKind::Duplicate));
        }
        let pid_present = match fields[0] {
            "-" => false,
            value => value.parse::<u32>().ok().is_some_and(|pid| pid > 0),
        };
        if fields[0] != "-" && !pid_present {
            return Err(error(LaunchdParseErrorKind::Malformed));
        }
        let last_exit_status = fields[1]
            .parse::<i32>()
            .map_err(|_| error(LaunchdParseErrorKind::Malformed))?;
        services.push(LaunchdService {
            service_label: label.to_owned(),
            pid_present,
            last_exit_status,
        });
        if services.len() > MAX_LAUNCHD_OUTPUTS {
            return Err(error(LaunchdParseErrorKind::RowLimit));
        }
    }
    services.sort_by(|left, right| left.service_label.cmp(&right.service_label));
    Ok(services)
}

pub fn map_launchd_services(
    host_identity: &HostIdentity,
    scope: &Scope,
    observed_at: Timestamp,
    input: &[u8],
    allowlist: &[ServiceId],
) -> Result<(Vec<ResourceObservation>, Vec<RelationObservation>), LaunchdParseError> {
    let services = parse_launchd_services(input, allowlist)?;
    let host = ResourceLocator {
        kind: kind("ssh.host"),
        external_id: host_identity.external_id(),
    };
    let mut resources = Vec::with_capacity(services.len());
    let mut relations = Vec::with_capacity(services.len());
    for service in services {
        let external_id = ExternalId::new(format!(
            "ssh-launchd-service:v1:{}:{}",
            host_identity.as_str(),
            service.service_label
        ))
        .expect("validated launchd external ID");
        let health = match (service.last_exit_status, service.pid_present) {
            (0, true) => ResourceHealth::Healthy,
            (0, false) => ResourceHealth::Unknown,
            _ => ResourceHealth::Degraded,
        };
        resources.push(ResourceObservation {
            kind: kind("ssh.launchd-service"),
            external_id: external_id.clone(),
            name: service.service_label.clone(),
            display_name: service.service_label.clone(),
            scope: scope.clone(),
            labels: labels(),
            health,
            attributes: json!({
                "service_label": service.service_label,
                "loaded": true,
                "pid_present": service.pid_present,
                "last_exit_status": service.last_exit_status,
            }),
            attribute_schema_version: SchemaVersion::new(1).expect("static schema"),
            observed_at,
        });
        relations.push(RelationObservation {
            source: host.clone(),
            target: ResourceLocator {
                kind: kind("ssh.launchd-service"),
                external_id,
            },
            kind: RelationKind::new("ssh.contains").expect("static relation kind"),
            evidence_key: EvidenceKey::new(format!(
                "ssh-provider-launchd:{}:{}",
                host_identity.as_str(),
                service.service_label
            ))
            .expect("validated launchd evidence key"),
            field_path: FieldPath::new("attributes.service_label").expect("static field path"),
            observed_at,
        });
    }
    Ok((resources, relations))
}

fn checked_text(input: &[u8]) -> Result<&str, LaunchdParseError> {
    if input.len() > MAX_LAUNCHD_STDOUT_BYTES {
        return Err(error(LaunchdParseErrorKind::OutputLimit));
    }
    let text = std::str::from_utf8(input).map_err(|_| error(LaunchdParseErrorKind::InvalidUtf8))?;
    if text
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err(error(LaunchdParseErrorKind::UnsafeOutput));
    }
    let lower = text.to_ascii_lowercase();
    if [
        "bearer ",
        "authorization:",
        "token=",
        "password=",
        "secret=",
        "-----begin",
        "private key",
    ]
    .iter()
    .any(|sentinel| lower.contains(sentinel))
    {
        return Err(error(LaunchdParseErrorKind::UnsafeOutput));
    }
    Ok(text)
}

fn labels() -> BTreeMap<LabelKey, String> {
    BTreeMap::from([
        (
            LabelKey::new("ssh.platform").expect("static label"),
            "darwin".into(),
        ),
        (
            LabelKey::new("ssh.resource_type").expect("static label"),
            "launchd-service".into(),
        ),
    ])
}

fn kind(value: &str) -> ResourceKind {
    ResourceKind::new(value).expect("static resource kind")
}

const fn error(kind: LaunchdParseErrorKind) -> LaunchdParseError {
    LaunchdParseError { kind }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDENTITY: &str = "9f7fd5e6-3bc8-4daa-ae6b-9dfdffb54743";

    fn allowed(values: &[&str]) -> Vec<ServiceId> {
        values
            .iter()
            .map(|value| ServiceId::parse(value).unwrap())
            .collect()
    }

    #[test]
    fn parses_allowlisted_running_on_demand_and_failed_services() {
        let services = parse_launchd_services(
            b"PID Status Label\n123 0 app.running\n- 0 app.ondemand\n- 78 app.failed\n999 0 ignored.service\n",
            &allowed(&["app.running", "app.ondemand", "app.failed", "missing.service"]),
        )
        .unwrap();
        assert_eq!(services.len(), 3);
        assert_eq!(services[0].last_exit_status, 78);
        assert!(!services[1].pid_present);
        assert!(services[2].pid_present);
    }

    #[test]
    fn missing_is_empty_and_unallowlisted_malformed_rows_are_discarded() {
        let services = parse_launchd_services(
            b"PID Status Label\nnot-a-pid not-a-status ignored.service\n",
            &allowed(&["missing.service"]),
        )
        .unwrap();
        assert!(services.is_empty());
    }

    #[test]
    fn duplicate_malformed_caps_and_sentinels_fail_redacted() {
        let allowlist = allowed(&["app.service"]);
        assert_eq!(
            parse_launchd_services(
                b"PID Status Label\n1 0 app.service\n2 0 app.service\n",
                &allowlist,
            )
            .unwrap_err()
            .kind,
            LaunchdParseErrorKind::Duplicate
        );
        assert!(
            parse_launchd_services(b"PID Status Label\n0 0 app.service\n", &allowlist).is_err()
        );
        let error = parse_launchd_services(
            b"PID Status Label\n1 0 app.service Bearer fixture-secret\n",
            &allowlist,
        )
        .unwrap_err();
        assert_eq!(error.kind, LaunchdParseErrorKind::UnsafeOutput);
        assert!(!format!("{error:?}").contains("fixture-secret"));

        let mut rows = String::from("PID Status Label\n");
        for _ in 0..=MAX_LAUNCHD_ROWS {
            rows.push_str("- 0 ignored.service\n");
        }
        assert_eq!(
            parse_launchd_services(rows.as_bytes(), &[])
                .unwrap_err()
                .kind,
            LaunchdParseErrorKind::RowLimit
        );
    }

    #[test]
    fn maps_stable_resources_relations_health_and_discards_unlisted_rows() {
        let identity = HostIdentity::parse(IDENTITY).unwrap();
        let (resources, relations) = map_launchd_services(
            &identity,
            &Scope::new("fixture-scope").unwrap(),
            Timestamp::from_unix_millis(1_000).unwrap(),
            b"PID Status Label\n123 0 app.running\n- 0 app.ondemand\n- -9 app.failed\n55 0 private.description\n",
            &allowed(&["app.running", "app.ondemand", "app.failed"]),
        )
        .unwrap();
        assert_eq!(resources.len(), 3);
        assert_eq!(relations.len(), 3);
        assert_eq!(resources[0].health, ResourceHealth::Degraded);
        assert_eq!(resources[1].health, ResourceHealth::Unknown);
        assert_eq!(resources[2].health, ResourceHealth::Healthy);
        assert!(
            relations
                .iter()
                .all(|relation| relation.field_path.as_str() == "attributes.service_label")
        );
        let serialized = serde_json::to_string(&(resources, relations)).unwrap();
        assert!(!serialized.contains("private.description"));
        assert!(serialized.contains(&format!("ssh-provider-launchd:{IDENTITY}:app.running")));
    }
}
