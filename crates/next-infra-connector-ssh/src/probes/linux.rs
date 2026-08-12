//! Bounded parser and mapper for the fixed Linux systemd probe.

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

pub const MAX_SYSTEMD_STDOUT_BYTES: usize = 512 * 1024;
pub const MAX_SYSTEMD_ROWS: usize = 2_048;
pub const MAX_SYSTEMD_OUTPUTS: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemdService {
    pub unit: String,
    pub load_state: String,
    pub active_state: String,
    pub sub_state: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemdParseErrorKind {
    InvalidUtf8,
    UnsafeOutput,
    OutputLimit,
    Malformed,
    Duplicate,
    RowLimit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SystemdParseError {
    pub kind: SystemdParseErrorKind,
}

impl SystemdParseError {
    pub fn connector_failure(self) -> ConnectorFailure {
        ConnectorFailure {
            code: ErrorCode::InvalidResponse,
            message: "SSH systemd probe response is invalid".into(),
            retryable: false,
            retry_after_ms: None,
        }
    }
}

impl fmt::Display for SystemdParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SSH systemd probe response is invalid")
    }
}

impl std::error::Error for SystemdParseError {}

/// `Some(allowlist)`: strict sync contract (malformed/duplicate/overflow fail).
/// `None`: best-effort discovery (skip bad rows, truncate at the output cap).
pub fn parse_systemd_services(
    input: &[u8],
    allowlist: Option<&[ServiceId]>,
) -> Result<Vec<SystemdService>, SystemdParseError> {
    let text = checked_text(input)?;
    let allowed = allowlist.map(|ids| ids.iter().map(ServiceId::expose).collect::<BTreeSet<_>>());
    let discovery = allowlist.is_none();
    let mut seen = BTreeSet::new();
    let mut services = Vec::new();
    for (row, line) in text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .enumerate()
    {
        if row >= MAX_SYSTEMD_ROWS {
            return Err(error(SystemdParseErrorKind::RowLimit));
        }
        let mut fields = line.split_ascii_whitespace();
        let Some(unit) = fields.next() else {
            continue;
        };
        if let Some(allowed) = &allowed
            && !allowed.contains(unit)
        {
            continue;
        }
        let load_state = fields.next();
        let active_state = fields.next();
        let sub_state = fields.next();
        let Some((load_state, active_state, sub_state)) = load_state
            .zip(active_state)
            .zip(sub_state)
            .map(|((a, b), c)| (a, b, c))
        else {
            if discovery {
                continue;
            }
            return Err(error(SystemdParseErrorKind::Malformed));
        };
        let well_formed = unit.len() <= 128
            && unit.ends_with(".service")
            && ServiceId::parse(unit).is_ok()
            && valid_state(load_state)
            && valid_state(active_state)
            && valid_state(sub_state);
        if !well_formed {
            if discovery {
                continue;
            }
            return Err(error(SystemdParseErrorKind::Malformed));
        }
        if !seen.insert(unit) {
            if discovery {
                continue;
            }
            return Err(error(SystemdParseErrorKind::Duplicate));
        }
        if services.len() >= MAX_SYSTEMD_OUTPUTS && discovery {
            break;
        }
        services.push(SystemdService {
            unit: unit.to_owned(),
            load_state: load_state.to_owned(),
            active_state: active_state.to_owned(),
            sub_state: sub_state.to_owned(),
        });
        if services.len() > MAX_SYSTEMD_OUTPUTS {
            return Err(error(SystemdParseErrorKind::RowLimit));
        }
    }
    services.sort_by(|left, right| left.unit.cmp(&right.unit));
    Ok(services)
}

pub fn map_systemd_services(
    host_identity: &HostIdentity,
    scope: &Scope,
    observed_at: Timestamp,
    input: &[u8],
    allowlist: &[ServiceId],
) -> Result<(Vec<ResourceObservation>, Vec<RelationObservation>), SystemdParseError> {
    let services = parse_systemd_services(input, Some(allowlist))?;
    let host = ResourceLocator {
        kind: kind("ssh.host"),
        external_id: host_identity.external_id(),
    };
    let mut resources = Vec::with_capacity(services.len());
    let mut relations = Vec::with_capacity(services.len());
    for service in services {
        let external_id = ExternalId::new(format!(
            "ssh-systemd-service:v1:{}:{}",
            host_identity.as_str(),
            service.unit
        ))
        .expect("validated systemd external ID");
        let health = match service.active_state.as_str() {
            "active" => ResourceHealth::Healthy,
            "failed" => ResourceHealth::Unhealthy,
            "activating" | "deactivating" | "reloading" => ResourceHealth::Degraded,
            _ => ResourceHealth::Unknown,
        };
        resources.push(ResourceObservation {
            kind: kind("ssh.systemd-service"),
            external_id: external_id.clone(),
            name: service.unit.clone(),
            display_name: service.unit.clone(),
            scope: scope.clone(),
            labels: labels(),
            health,
            attributes: json!({
                "unit": service.unit,
                "load_state": service.load_state,
                "active_state": service.active_state,
                "sub_state": service.sub_state,
            }),
            attribute_schema_version: SchemaVersion::new(1).expect("static schema"),
            observed_at,
        });
        relations.push(RelationObservation {
            source: host.clone(),
            target: ResourceLocator {
                kind: kind("ssh.systemd-service"),
                external_id,
            },
            kind: RelationKind::new("ssh.contains").expect("static relation kind"),
            evidence_key: EvidenceKey::new(format!(
                "ssh-provider-systemd:{}:{}",
                host_identity.as_str(),
                service.unit
            ))
            .expect("validated systemd evidence key"),
            field_path: FieldPath::new("attributes.unit").expect("static field path"),
            observed_at,
        });
    }
    Ok((resources, relations))
}

fn checked_text(input: &[u8]) -> Result<&str, SystemdParseError> {
    if input.len() > MAX_SYSTEMD_STDOUT_BYTES {
        return Err(error(SystemdParseErrorKind::OutputLimit));
    }
    let text = std::str::from_utf8(input).map_err(|_| error(SystemdParseErrorKind::InvalidUtf8))?;
    if text
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err(error(SystemdParseErrorKind::UnsafeOutput));
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
        return Err(error(SystemdParseErrorKind::UnsafeOutput));
    }
    Ok(text)
}

fn valid_state(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'-')
}

fn labels() -> BTreeMap<LabelKey, String> {
    BTreeMap::from([
        (
            LabelKey::new("ssh.platform").expect("static label"),
            "linux".into(),
        ),
        (
            LabelKey::new("ssh.resource_type").expect("static label"),
            "systemd-service".into(),
        ),
    ])
}

fn kind(value: &str) -> ResourceKind {
    ResourceKind::new(value).expect("static resource kind")
}

const fn error(kind: SystemdParseErrorKind) -> SystemdParseError {
    SystemdParseError { kind }
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
    fn parses_allowlisted_states_and_discards_descriptions() {
        let services = parse_systemd_services(
            b"app-active.service loaded active running Visible description\napp-failed.service loaded failed failed Failure details\nignored.service loaded active running Private description\n",
            Some(&allowed(&["app-active.service", "app-failed.service", "missing.service"])),
        )
        .unwrap();
        assert_eq!(services.len(), 2);
        assert_eq!(services[0].active_state, "active");
        assert_eq!(services[1].active_state, "failed");
        assert!(!format!("{services:?}").contains("description"));
    }

    #[test]
    fn missing_is_empty_and_unallowlisted_malformed_rows_are_discarded() {
        let services = parse_systemd_services(
            b"ignored.service INVALID INVALID INVALID hidden\n",
            Some(&allowed(&["missing.service"])),
        )
        .unwrap();
        assert!(services.is_empty());
    }

    #[test]
    fn discovery_skips_bad_rows_and_truncates_at_the_output_cap() {
        let services = parse_systemd_services(
            b"app-active.service loaded active running one\nignored.socket loaded active running skip\napp-dead.service loaded inactive dead two\n",
            None,
        )
        .unwrap();
        assert_eq!(
            services
                .iter()
                .map(|service| service.unit.as_str())
                .collect::<Vec<_>>(),
            ["app-active.service", "app-dead.service"]
        );

        let mut rows = String::new();
        for index in 0..(MAX_SYSTEMD_OUTPUTS + 8) {
            rows.push_str(&format!(
                "cap-{index}.service loaded active running hidden\n"
            ));
        }
        let capped = parse_systemd_services(rows.as_bytes(), None).unwrap();
        assert_eq!(capped.len(), MAX_SYSTEMD_OUTPUTS);
    }

    #[test]
    fn duplicate_malformed_caps_and_sentinels_fail_redacted() {
        let allowlist = allowed(&["app.service"]);
        assert_eq!(
            parse_systemd_services(
                b"app.service loaded active running one\napp.service loaded inactive dead two\n",
                Some(&allowlist),
            )
            .unwrap_err()
            .kind,
            SystemdParseErrorKind::Duplicate
        );
        assert!(
            parse_systemd_services(b"app.service LOADED active running\n", Some(&allowlist))
                .is_err()
        );
        let error = parse_systemd_services(
            b"app.service loaded active running Bearer fixture-secret\n",
            Some(&allowlist),
        )
        .unwrap_err();
        assert_eq!(error.kind, SystemdParseErrorKind::UnsafeOutput);
        assert!(!format!("{error:?}").contains("fixture-secret"));

        let mut rows = String::new();
        for _ in 0..=MAX_SYSTEMD_ROWS {
            rows.push_str("ignored.service loaded inactive dead hidden\n");
        }
        assert_eq!(
            parse_systemd_services(rows.as_bytes(), Some(&[]))
                .unwrap_err()
                .kind,
            SystemdParseErrorKind::RowLimit
        );
    }

    #[test]
    fn maps_stable_resources_relations_and_health() {
        let identity = HostIdentity::parse(IDENTITY).unwrap();
        let (resources, relations) = map_systemd_services(
            &identity,
            &Scope::new("fixture-scope").unwrap(),
            Timestamp::from_unix_millis(1_000).unwrap(),
            b"active.service loaded active running live\nfailed.service loaded failed failed failed\nstarting.service loaded activating start-pre starting\ninactive.service loaded inactive dead idle\n",
            &allowed(&["active.service", "failed.service", "starting.service", "inactive.service"]),
        )
        .unwrap();
        assert_eq!(resources.len(), 4);
        assert_eq!(relations.len(), 4);
        assert_eq!(resources[0].health, ResourceHealth::Healthy);
        assert_eq!(resources[1].health, ResourceHealth::Unhealthy);
        assert_eq!(resources[2].health, ResourceHealth::Unknown);
        assert_eq!(resources[3].health, ResourceHealth::Degraded);
        assert!(
            relations
                .iter()
                .all(|relation| relation.field_path.as_str() == "attributes.unit")
        );
        let serialized = serde_json::to_string(&(resources, relations)).unwrap();
        assert!(serialized.contains(&format!("ssh-provider-systemd:{IDENTITY}:active.service")));
        assert!(!serialized.contains(" idle"));
    }
}
