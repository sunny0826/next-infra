use crate::{MAX_PROBE_STDERR_BYTES, MAX_PROBE_WALL_TIME_SECS, SshError};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, time::Duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ProbeId {
    #[serde(rename = "host.identity.v1")]
    HostIdentityV1,
    #[serde(rename = "host.uptime.v1")]
    HostUptimeV1,
    #[serde(rename = "host.filesystems.v1")]
    HostFilesystemsV1,
    #[serde(rename = "host.process_summary.v1")]
    HostProcessSummaryV1,
    #[serde(rename = "macos.launchd_services.v1")]
    MacosLaunchdServicesV1,
    #[serde(rename = "linux.systemd_services.v1")]
    LinuxSystemdServicesV1,
}

impl ProbeId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HostIdentityV1 => "host.identity.v1",
            Self::HostUptimeV1 => "host.uptime.v1",
            Self::HostFilesystemsV1 => "host.filesystems.v1",
            Self::HostProcessSummaryV1 => "host.process_summary.v1",
            Self::MacosLaunchdServicesV1 => "macos.launchd_services.v1",
            Self::LinuxSystemdServicesV1 => "linux.systemd_services.v1",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbePlatform {
    All,
    Macos,
    Linux,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct ProbeMetadata {
    pub id: ProbeId,
    pub schema_version: u32,
    pub platform: ProbePlatform,
    pub timeout_secs: u64,
    pub stdout_limit_bytes: usize,
    pub stderr_limit_bytes: usize,
    pub parser_owner: &'static str,
}

#[derive(Clone, Copy)]
pub(crate) struct ProbeSpec {
    pub metadata: ProbeMetadata,
    pub command: &'static str,
}

const IDENTITY_COMMAND: &str = "LC_ALL=C PATH=/usr/bin:/bin:/usr/sbin:/sbin uname -s; LC_ALL=C PATH=/usr/bin:/bin:/usr/sbin:/sbin uname -m";
const UPTIME_COMMAND: &str = "LC_ALL=C PATH=/usr/bin:/bin:/usr/sbin:/sbin uptime";
const FILESYSTEMS_COMMAND: &str = "LC_ALL=C PATH=/usr/bin:/bin:/usr/sbin:/sbin df -Pk";
const PROCESS_SUMMARY_COMMAND: &str =
    "LC_ALL=C PATH=/usr/bin:/bin:/usr/sbin:/sbin ps -Ao state=,comm=";
const LAUNCHD_COMMAND: &str = "LC_ALL=C PATH=/usr/bin:/bin:/usr/sbin:/sbin launchctl list";
const SYSTEMD_COMMAND: &str = "LC_ALL=C PATH=/usr/bin:/bin:/usr/sbin:/sbin systemctl list-units --type=service --all --no-pager --no-legend --plain";

const REGISTRY: [ProbeSpec; 6] = [
    spec(
        ProbeId::HostIdentityV1,
        ProbePlatform::All,
        10,
        64 * 1024,
        "CON-G6-02",
        IDENTITY_COMMAND,
    ),
    spec(
        ProbeId::HostUptimeV1,
        ProbePlatform::All,
        10,
        16 * 1024,
        "CON-G6-02",
        UPTIME_COMMAND,
    ),
    spec(
        ProbeId::HostFilesystemsV1,
        ProbePlatform::All,
        15,
        256 * 1024,
        "CON-G6-02",
        FILESYSTEMS_COMMAND,
    ),
    spec(
        ProbeId::HostProcessSummaryV1,
        ProbePlatform::All,
        15,
        256 * 1024,
        "CON-G6-02",
        PROCESS_SUMMARY_COMMAND,
    ),
    spec(
        ProbeId::MacosLaunchdServicesV1,
        ProbePlatform::Macos,
        20,
        512 * 1024,
        "CON-G6-03",
        LAUNCHD_COMMAND,
    ),
    spec(
        ProbeId::LinuxSystemdServicesV1,
        ProbePlatform::Linux,
        20,
        512 * 1024,
        "CON-G6-04",
        SYSTEMD_COMMAND,
    ),
];

const fn spec(
    id: ProbeId,
    platform: ProbePlatform,
    timeout_secs: u64,
    stdout_limit_bytes: usize,
    parser_owner: &'static str,
    command: &'static str,
) -> ProbeSpec {
    ProbeSpec {
        metadata: ProbeMetadata {
            id,
            schema_version: 1,
            platform,
            timeout_secs,
            stdout_limit_bytes,
            stderr_limit_bytes: MAX_PROBE_STDERR_BYTES,
            parser_owner,
        },
        command,
    }
}

pub fn probe_registry() -> Vec<ProbeMetadata> {
    REGISTRY.iter().map(|entry| entry.metadata).collect()
}

pub fn probe_metadata(id: ProbeId) -> ProbeMetadata {
    probe_spec(id)
        .expect("all ProbeId variants are registered")
        .metadata
}

pub(crate) fn probe_spec(id: ProbeId) -> Result<&'static ProbeSpec, SshError> {
    REGISTRY
        .iter()
        .find(|entry| entry.metadata.id == id)
        .ok_or_else(SshError::invalid_config)
}

pub(crate) fn validate_registry() -> Result<(), SshError> {
    let unique = REGISTRY
        .iter()
        .map(|entry| entry.metadata.id)
        .collect::<BTreeSet<_>>();
    if unique.len() != REGISTRY.len()
        || REGISTRY.iter().any(|entry| {
            entry.command.is_empty()
                || entry.metadata.schema_version != 1
                || entry.metadata.timeout_secs > MAX_PROBE_WALL_TIME_SECS
                || entry.metadata.stderr_limit_bytes > MAX_PROBE_STDERR_BYTES
        })
    {
        return Err(SshError::invalid_config());
    }
    Ok(())
}

pub(crate) fn timeout_for(id: ProbeId) -> Duration {
    Duration::from_secs(probe_metadata(id).timeout_secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_unique_bounded_and_commands_are_final() {
        validate_registry().unwrap();
        assert_eq!(probe_registry().len(), 6);
        assert_eq!(
            probe_spec(ProbeId::HostIdentityV1).unwrap().command,
            IDENTITY_COMMAND
        );
        assert_eq!(
            probe_spec(ProbeId::LinuxSystemdServicesV1).unwrap().command,
            SYSTEMD_COMMAND
        );
        let metadata = serde_json::to_string(&probe_registry()).unwrap();
        assert!(!metadata.contains("uname"));
        assert!(!metadata.contains("systemctl"));
    }
}
