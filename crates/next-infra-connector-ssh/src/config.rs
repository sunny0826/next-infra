use crate::SshError;
use next_infra_core::ExternalId;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{collections::BTreeSet, fmt, str::FromStr};
use uuid::Uuid;

/// `connect_timeout_secs` accepted range, aligned with the Host-side connect
/// timeout validation (5..=14 seconds) so any config that passes connection
/// setup also passes every sync-time config validation.
const MIN_CONNECT_TIMEOUT_SECS: u8 = 5;
const DEFAULT_CONNECT_TIMEOUT_SECS: u8 = 10;
const MAX_SERVICE_IDS: usize = 64;
const MAX_SERVICE_ID_BYTES: usize = 128;

#[derive(Clone, PartialEq, Eq)]
pub struct HostIdentity(Uuid);

impl HostIdentity {
    pub fn generate() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn parse(value: &str) -> Result<Self, SshError> {
        let parsed = Uuid::parse_str(value).map_err(|_| SshError::invalid_config())?;
        if parsed.get_version_num() != 4 || parsed.hyphenated().to_string() != value {
            return Err(SshError::invalid_config());
        }
        Ok(Self(parsed))
    }

    pub fn external_id(&self) -> ExternalId {
        ExternalId::new(format!("ssh-host:v1:{}", self.0.hyphenated()))
            .expect("canonical UUID external ID is valid")
    }

    pub fn as_str(&self) -> String {
        self.0.hyphenated().to_string()
    }
}

impl fmt::Debug for HostIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("HostIdentity([REDACTED])")
    }
}

impl<'de> Deserialize<'de> for HostIdentity {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

impl Serialize for HostIdentity {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.hyphenated().to_string())
    }
}

#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct HostAlias(String);

impl HostAlias {
    pub fn parse(value: &str) -> Result<Self, SshError> {
        if value.is_empty()
            || value.len() > 128
            || !value.bytes().enumerate().all(|(index, byte)| {
                byte.is_ascii_alphanumeric() || (index > 0 && matches!(byte, b'.' | b'_' | b'-'))
            })
        {
            return Err(SshError::invalid_config());
        }
        Ok(Self(value.to_owned()))
    }

    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for HostAlias {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("HostAlias([REDACTED])")
    }
}

impl<'de> Deserialize<'de> for HostAlias {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProbeProfile {
    #[serde(rename = "baseline-v1")]
    BaselineV1,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct ServiceId(String);

impl ServiceId {
    pub fn parse(value: &str) -> Result<Self, SshError> {
        if value.is_empty()
            || value.len() > MAX_SERVICE_ID_BYTES
            || !value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'@')
            })
        {
            return Err(SshError::invalid_config());
        }
        Ok(Self(value.to_owned()))
    }

    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ServiceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ServiceId([REDACTED])")
    }
}

impl<'de> Deserialize<'de> for ServiceId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SshConnectionConfigV1 {
    pub host_identity: HostIdentity,
    pub host_alias: HostAlias,
    #[serde(default = "default_connect_timeout_secs")]
    pub connect_timeout_secs: u8,
    pub probe_profile: ProbeProfile,
    #[serde(default)]
    pub allowed_service_ids: Vec<ServiceId>,
}

impl SshConnectionConfigV1 {
    pub fn from_json(value: serde_json::Value) -> Result<Self, SshError> {
        let config: Self = serde_json::from_value(value).map_err(|_| SshError::invalid_config())?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), SshError> {
        if !(MIN_CONNECT_TIMEOUT_SECS..=crate::MAX_CONNECT_TIMEOUT_SECS)
            .contains(&self.connect_timeout_secs)
            || self.allowed_service_ids.len() > MAX_SERVICE_IDS
            || self
                .allowed_service_ids
                .iter()
                .collect::<BTreeSet<_>>()
                .len()
                != self.allowed_service_ids.len()
        {
            return Err(SshError::invalid_config());
        }
        Ok(())
    }
}

impl fmt::Debug for SshConnectionConfigV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SshConnectionConfigV1")
            .field("host_identity", &"[REDACTED]")
            .field("host_alias", &"[REDACTED]")
            .field("connect_timeout_secs", &self.connect_timeout_secs)
            .field("probe_profile", &self.probe_profile)
            .field("allowed_service_ids", &self.allowed_service_ids.len())
            .finish()
    }
}

fn default_connect_timeout_secs() -> u8 {
    DEFAULT_CONNECT_TIMEOUT_SECS
}

impl fmt::Display for HostIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0.hyphenated().to_string())
    }
}

impl FromStr for HostIdentity {
    type Err = SshError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn identity() -> &'static str {
        "9f7fd5e6-3bc8-4daa-ae6b-9dfdffb54743"
    }

    #[test]
    fn identity_is_v4_canonical_and_external_id_is_alias_independent() {
        let host_identity = HostIdentity::parse(identity()).unwrap();
        assert_eq!(
            host_identity.external_id().as_str(),
            "ssh-host:v1:9f7fd5e6-3bc8-4daa-ae6b-9dfdffb54743"
        );
        assert!(HostIdentity::parse("9F7FD5E6-3BC8-4DAA-AE6B-9DFDFFB54743").is_err());
        assert!(HostIdentity::parse("00000000-0000-1000-8000-000000000000").is_err());
        assert_eq!(HostIdentity::generate().as_str().len(), 36);
    }

    #[test]
    fn alias_rejects_option_and_shell_injection() {
        for valid in ["mac-mini", "lab.host_1", "a"] {
            HostAlias::parse(valid).unwrap();
        }
        for invalid in [
            "",
            "-oProxyCommand",
            "host name",
            "host\nname",
            "host*",
            "host/name",
            "host:22",
            "host;id",
            "host$(id)",
        ] {
            assert!(HostAlias::parse(invalid).is_err(), "{invalid:?}");
        }
    }

    #[test]
    fn connect_timeout_accepts_host_aligned_range_and_rejects_outside() {
        for timeout in [5, 10, 14] {
            let value = json!({
                "host_identity": identity(),
                "host_alias": "fixture-host",
                "connect_timeout_secs": timeout,
                "probe_profile": "baseline-v1",
            });
            assert!(
                SshConnectionConfigV1::from_json(value).is_ok(),
                "connect_timeout_secs={timeout} should be accepted"
            );
        }
        for timeout in [0, 4, 15] {
            let value = json!({
                "host_identity": identity(),
                "host_alias": "fixture-host",
                "connect_timeout_secs": timeout,
                "probe_profile": "baseline-v1",
            });
            assert!(
                SshConnectionConfigV1::from_json(value).is_err(),
                "connect_timeout_secs={timeout} should be rejected"
            );
        }
    }

    #[test]
    fn config_is_allowlisted_bounded_and_redacted() {
        let value = json!({
            "host_identity": identity(),
            "host_alias": "fixture-host",
            "connect_timeout_secs": 10,
            "probe_profile": "baseline-v1",
            "allowed_service_ids": ["example.service"]
        });
        let config = SshConnectionConfigV1::from_json(value).unwrap();
        let debug = format!("{config:?}");
        assert!(!debug.contains("fixture-host"));
        assert!(!debug.contains(identity()));

        let unknown = json!({
            "host_identity": identity(),
            "host_alias": "fixture-host",
            "probe_profile": "baseline-v1",
            "command": "output-sentinel"
        });
        let error = SshConnectionConfigV1::from_json(unknown).unwrap_err();
        assert!(!format!("{error:?}").contains("output-sentinel"));

        let duplicate = json!({
            "host_identity": identity(),
            "host_alias": "fixture-host",
            "probe_profile": "baseline-v1",
            "allowed_service_ids": ["same", "same"]
        });
        assert!(SshConnectionConfigV1::from_json(duplicate).is_err());
    }
}
