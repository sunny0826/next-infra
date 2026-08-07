use next_infra_connector_api::{
    AuthDescriptor, AuthKind, ConnectorDescriptor, RateLimitGuidance, RelationCapability,
    ResourceCapability,
};
use next_infra_core::{
    ConnectorCoverage, ConnectorCoverageLevel, ConnectorType, RelationKind, ResourceKind,
    SchemaVersion, SyncMode,
};

/// Permission names are the minimal read grants documented by Cloudflare's
/// API-token permissions reference for the selected Account and Zone scopes.
pub fn cloudflare_descriptor() -> ConnectorDescriptor {
    let account = kind("cloudflare.account");
    let zone = kind("cloudflare.zone");
    let dns_record = kind("cloudflare.dns_record");
    let tunnel = kind("cloudflare.tunnel");
    let worker = kind("cloudflare.worker");
    ConnectorDescriptor {
        connector_type: ConnectorType::new("cloudflare").expect("static connector type"),
        connector_version: "1.0.0".into(),
        config_schema_version: SchemaVersion::new(1).expect("static schema version"),
        auth: AuthDescriptor {
            kind: AuthKind::Token,
            minimum_permissions: vec![
                "Account: Account Settings: Read".into(),
                "Account: Cloudflare Tunnel: Read".into(),
                "Account: Workers Scripts: Read".into(),
                "Zone: Zone: Read".into(),
                "Zone: DNS: Read".into(),
            ],
        },
        sync_modes: vec![SyncMode::Full, SyncMode::Targeted],
        resources: vec![
            resource(account.clone(), "cloudflare.accounts", ConnectorCoverageLevel::Supported, None),
            resource(zone.clone(), "cloudflare.zones", ConnectorCoverageLevel::Supported, None),
            resource(dns_record.clone(), "cloudflare.dns_records", ConnectorCoverageLevel::Supported, None),
            resource(tunnel.clone(), "cloudflare.tunnels", ConnectorCoverageLevel::Partial, Some("tunnels require account-scoped token access")),
            resource(worker.clone(), "cloudflare.workers", ConnectorCoverageLevel::Partial, Some("only script metadata is collected; worker code is excluded")),
        ],
        relations: vec![
            relation(&account, &zone, "cloudflare.contains", "cloudflare.account_zone"),
            relation(&zone, &dns_record, "cloudflare.contains", "cloudflare.zone_dns_record"),
            relation(&account, &tunnel, "cloudflare.contains", "cloudflare.account_tunnel"),
            relation(&account, &worker, "cloudflare.contains", "cloudflare.account_worker"),
        ],
        sensitive_field_policy: vec![
            "API tokens are accepted only as ephemeral SecretValue input".into(),
            "Worker script code, raw response bodies and token values are never persisted or logged".into(),
            "the token must be scoped to selected Account and Zone resources with read permissions only".into(),
        ],
        rate_limit: RateLimitGuidance { default_max_concurrency: 2, requests_per_minute: None, respects_retry_after: true },
        recommended_sync_interval_secs: 900,
        known_gaps: vec![
            "Worker code, routes that expose source, write APIs and unscoped global API keys are unsupported".into(),
            "modules without the listed read permission report partial coverage rather than deleting prior observations".into(),
        ],
    }
}

fn kind(value: &str) -> ResourceKind {
    ResourceKind::new(value).expect("static resource kind")
}

fn resource(
    kind: ResourceKind,
    module: &str,
    level: ConnectorCoverageLevel,
    reason: Option<&str>,
) -> ResourceCapability {
    ResourceCapability {
        kind,
        attribute_schema_version: SchemaVersion::new(1).expect("static schema version"),
        coverage: ConnectorCoverage {
            module: module.into(),
            level,
            reason: reason.map(str::to_owned),
        },
    }
}

fn relation(
    source_kind: &ResourceKind,
    target_kind: &ResourceKind,
    relation_kind: &str,
    module: &str,
) -> RelationCapability {
    RelationCapability {
        kind: RelationKind::new(relation_kind).expect("static relation kind"),
        source_kind: source_kind.clone(),
        target_kind: target_kind.clone(),
        coverage: ConnectorCoverage {
            module: module.into(),
            level: ConnectorCoverageLevel::Supported,
            reason: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use next_infra_connector_contract_tests::check_descriptor;

    #[test]
    fn descriptor_is_valid_and_requires_scoped_read_permissions() {
        let descriptor = cloudflare_descriptor();
        assert!(descriptor.validate().is_ok());
        assert!(check_descriptor(&descriptor).is_empty());
        assert!(
            descriptor
                .auth
                .minimum_permissions
                .iter()
                .any(|permission| permission == "Zone: DNS: Read")
        );
        assert!(
            descriptor
                .auth
                .minimum_permissions
                .iter()
                .all(|permission| permission.contains("Read"))
        );
    }

    #[test]
    fn descriptor_excludes_worker_code_and_global_keys() {
        let serialized = serde_json::to_string(&cloudflare_descriptor())
            .unwrap()
            .to_ascii_lowercase();
        assert!(serialized.contains("worker code"));
        assert!(serialized.contains("global api keys are unsupported"));
        assert!(serialized.contains("never persisted"));
    }
}
