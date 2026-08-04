use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::cmp::Ordering;
use std::fmt;

use super::error::RpcError;

pub const PROTOCOL_MAJOR: u16 = 1;
pub const PROTOCOL_MINOR: u16 = 0;
pub const MINIMUM_SUPPORTED_MINOR: u16 = 0;

/// The only capabilities admitted by Local RPC v1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Capability {
    GetHealthSummary,
    GetRecentChanges,
    GetResource,
    GetSyncStatus,
    GetTopology,
    ListConnectorCoverage,
    SearchResources,
}

impl Capability {
    pub const ALL: [Self; 7] = [
        Self::GetHealthSummary,
        Self::GetRecentChanges,
        Self::GetResource,
        Self::GetSyncStatus,
        Self::GetTopology,
        Self::ListConnectorCoverage,
        Self::SearchResources,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GetHealthSummary => "query.get_health_summary.v1",
            Self::GetRecentChanges => "query.get_recent_changes.v1",
            Self::GetResource => "query.get_resource.v1",
            Self::GetSyncStatus => "query.get_sync_status.v1",
            Self::GetTopology => "query.get_topology.v1",
            Self::ListConnectorCoverage => "query.list_connector_coverage.v1",
            Self::SearchResources => "query.search_resources.v1",
        }
    }
}

impl Ord for Capability {
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_str().cmp(other.as_str())
    }
}

impl PartialOrd for Capability {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Serialize for Capability {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Capability {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.as_str() {
            "query.get_health_summary.v1" => Ok(Self::GetHealthSummary),
            "query.get_recent_changes.v1" => Ok(Self::GetRecentChanges),
            "query.get_resource.v1" => Ok(Self::GetResource),
            "query.get_sync_status.v1" => Ok(Self::GetSyncStatus),
            "query.get_topology.v1" => Ok(Self::GetTopology),
            "query.list_connector_coverage.v1" => Ok(Self::ListConnectorCoverage),
            "query.search_resources.v1" => Ok(Self::SearchResources),
            _ => Err(serde::de::Error::custom(format!(
                "unknown Local RPC capability: {value}"
            ))),
        }
    }
}

/// Deterministically encoded, duplicate-free capability set.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct CapabilitySet(Vec<Capability>);

impl CapabilitySet {
    pub fn new<I>(capabilities: I) -> Result<Self, RpcError>
    where
        I: IntoIterator<Item = Capability>,
    {
        let mut values = capabilities.into_iter().collect::<Vec<_>>();
        values.sort();
        if values.windows(2).any(|window| window[0] == window[1]) {
            return Err(RpcError::capability_mismatch(
                "capability sets must not contain duplicate entries",
            ));
        }
        Ok(Self(values))
    }

    pub fn empty() -> Self {
        Self::default()
    }

    pub fn all_query_capabilities() -> Self {
        // ALL contains every variant exactly once, so construction cannot fail.
        Self::new(Capability::ALL).expect("the frozen capability list is unique")
    }

    pub fn contains(&self, capability: Capability) -> bool {
        self.0.binary_search(&capability).is_ok()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = Capability> + '_ {
        self.0.iter().copied()
    }

    pub fn as_slice(&self) -> &[Capability] {
        &self.0
    }
}

impl FromIterator<Capability> for CapabilitySet {
    fn from_iter<T: IntoIterator<Item = Capability>>(iter: T) -> Self {
        // BTreeSet-like collection is useful in tests and constructors.  A
        // duplicate cannot be represented in this infallible conversion, so
        // it is rejected by `new` in all wire/deserialization paths instead.
        let mut values = iter.into_iter().collect::<Vec<_>>();
        values.sort();
        values.dedup();
        Self(values)
    }
}

impl Serialize for CapabilitySet {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for CapabilitySet {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let values = Vec::<Capability>::deserialize(deserializer)?;
        let mut sorted = values.clone();
        sorted.sort();
        if sorted.windows(2).any(|window| window[0] == window[1]) {
            return Err(serde::de::Error::custom(
                "capability sets must not contain duplicate entries",
            ));
        }
        Ok(Self(sorted))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientHello {
    pub protocol_major: u16,
    pub protocol_minor: u16,
    pub minimum_supported_minor: u16,
    pub bridge_version: String,
    pub release_id: String,
    pub supported_capabilities: CapabilitySet,
    pub required_capabilities: CapabilitySet,
}

impl ClientHello {
    pub fn new(
        protocol_minor: u16,
        minimum_supported_minor: u16,
        bridge_version: impl Into<String>,
        release_id: impl Into<String>,
        supported_capabilities: CapabilitySet,
        required_capabilities: CapabilitySet,
    ) -> Self {
        Self {
            protocol_major: PROTOCOL_MAJOR,
            protocol_minor,
            minimum_supported_minor,
            bridge_version: bridge_version.into(),
            release_id: release_id.into(),
            supported_capabilities,
            required_capabilities,
        }
    }

    pub fn initial(bridge_version: impl Into<String>, release_id: impl Into<String>) -> Self {
        Self::new(
            PROTOCOL_MINOR,
            MINIMUM_SUPPORTED_MINOR,
            bridge_version,
            release_id,
            CapabilitySet::empty(),
            CapabilitySet::all_query_capabilities(),
        )
    }

    pub fn validate(&self) -> Result<(), RpcError> {
        validate_range(
            self.protocol_major,
            self.protocol_minor,
            self.minimum_supported_minor,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostHello {
    pub protocol_major: u16,
    pub protocol_minor: u16,
    pub minimum_supported_minor: u16,
    pub selected_protocol_minor: u16,
    pub host_version: String,
    pub release_id: String,
    pub supported_capabilities: CapabilitySet,
    pub required_capabilities: CapabilitySet,
}

impl HostHello {
    pub fn new(
        protocol_minor: u16,
        minimum_supported_minor: u16,
        selected_protocol_minor: u16,
        host_version: impl Into<String>,
        release_id: impl Into<String>,
        supported_capabilities: CapabilitySet,
        required_capabilities: CapabilitySet,
    ) -> Self {
        Self {
            protocol_major: PROTOCOL_MAJOR,
            protocol_minor,
            minimum_supported_minor,
            selected_protocol_minor,
            host_version: host_version.into(),
            release_id: release_id.into(),
            supported_capabilities,
            required_capabilities,
        }
    }

    pub fn initial(host_version: impl Into<String>, release_id: impl Into<String>) -> Self {
        Self::new(
            PROTOCOL_MINOR,
            MINIMUM_SUPPORTED_MINOR,
            PROTOCOL_MINOR,
            host_version,
            release_id,
            CapabilitySet::all_query_capabilities(),
            CapabilitySet::empty(),
        )
    }

    pub fn validate(&self) -> Result<(), RpcError> {
        validate_range(
            self.protocol_major,
            self.protocol_minor,
            self.minimum_supported_minor,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandshakeResult {
    pub selected_protocol_minor: u16,
    pub upgrade_recommended: bool,
}

/// The first response sent by a Host after receiving ClientHello.
///
/// A rejected handshake has no request ID because request envelopes are not
/// admitted until this response is accepted.  The accepted variant carries
/// the authoritative HostHello (whose `selected_protocol_minor` is the
/// negotiated minor) and only the additional upgrade recommendation bit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum HandshakeResponse {
    Accepted {
        host: HostHello,
        upgrade_recommended: bool,
    },
    Rejected {
        error: RpcError,
    },
}

impl HandshakeResponse {
    pub fn accepted(host: HostHello, upgrade_recommended: bool) -> Self {
        Self::Accepted {
            host,
            upgrade_recommended,
        }
    }

    pub fn rejected(error: RpcError) -> Self {
        Self::Rejected { error }
    }

    pub fn selected_protocol_minor(&self) -> Option<u16> {
        match self {
            Self::Accepted { host, .. } => Some(host.selected_protocol_minor),
            Self::Rejected { .. } => None,
        }
    }
}

/// Negotiate a ClientHello and HostHello according to DEC-G1-03.
pub fn negotiate(client: &ClientHello, host: &HostHello) -> Result<HandshakeResult, RpcError> {
    if client.protocol_major != host.protocol_major {
        return Err(RpcError::protocol_mismatch(
            "client and host protocol majors differ",
        ));
    }
    client.validate()?;
    host.validate()?;

    let lower = client
        .minimum_supported_minor
        .max(host.minimum_supported_minor);
    let upper = client.protocol_minor.min(host.protocol_minor);
    if lower > upper {
        return Err(RpcError::protocol_mismatch(
            "client and host minor version ranges do not overlap",
        ));
    }
    if host.selected_protocol_minor != upper {
        return Err(RpcError::protocol_mismatch(
            "host selected a minor version other than the highest overlap",
        ));
    }

    if let Some(missing) = client
        .required_capabilities
        .iter()
        .find(|capability| !host.supported_capabilities.contains(*capability))
    {
        return Err(RpcError::capability_mismatch(format!(
            "host does not support required capability {}",
            missing.as_str()
        )));
    }
    if let Some(missing) = host
        .required_capabilities
        .iter()
        .find(|capability| !client.supported_capabilities.contains(*capability))
    {
        return Err(RpcError::capability_mismatch(format!(
            "client does not support host-required capability {}",
            missing.as_str()
        )));
    }

    Ok(HandshakeResult {
        selected_protocol_minor: upper,
        upgrade_recommended: client.release_id != host.release_id,
    })
}

/// Build the typed wire response for a ClientHello/HostHello exchange.
pub fn handshake_response(client: &ClientHello, host: &HostHello) -> HandshakeResponse {
    match negotiate(client, host) {
        Ok(result) => HandshakeResponse::accepted(host.clone(), result.upgrade_recommended),
        Err(error) => HandshakeResponse::rejected(error),
    }
}

fn validate_range(major: u16, minor: u16, minimum: u16) -> Result<(), RpcError> {
    if major != PROTOCOL_MAJOR || minimum > minor || minor.saturating_sub(minimum) > 1 {
        return Err(RpcError::protocol_mismatch(format!(
            "unsupported protocol range {major}.{minor} with minimum minor {minimum}"
        )));
    }
    Ok(())
}

impl fmt::Display for Capability {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}
