use next_infra_connector_api::{
    AuthDescriptor, AuthKind, ConnectorDescriptor, RateLimitGuidance, RelationCapability,
    ResourceCapability,
};
use next_infra_core::{
    ConnectorCoverage, ConnectorCoverageLevel, ConnectorType, RelationKind, ResourceKind,
    SchemaVersion, SyncMode,
};

pub fn ssh_descriptor() -> ConnectorDescriptor {
    let host = kind("ssh.host");
    let filesystem = kind("ssh.filesystem");
    let process_summary = kind("ssh.process-summary");
    let launchd_service = kind("ssh.launchd-service");
    let systemd_service = kind("ssh.systemd-service");

    ConnectorDescriptor {
        connector_type: ConnectorType::new("ssh").expect("static connector type"),
        connector_version: "1.0.0".into(),
        config_schema_version: SchemaVersion::new(1).expect("static schema version"),
        auth: AuthDescriptor {
            kind: AuthKind::SshAgent,
            minimum_permissions: vec![
                "existing SSH config alias".into(),
                "non-root permission to run the registered read-only probes".into(),
            ],
        },
        sync_modes: vec![SyncMode::Full, SyncMode::Targeted],
        resources: vec![
            resource(host.clone(), "ssh.host"),
            resource(filesystem.clone(), "ssh.filesystems"),
            resource(process_summary.clone(), "ssh.process-summary"),
            resource(launchd_service.clone(), "ssh.launchd-services"),
            resource(systemd_service.clone(), "ssh.systemd-services"),
        ],
        relations: vec![
            relation(&host, &filesystem, "ssh.host-filesystem"),
            relation(&host, &process_summary, "ssh.host-process-summary"),
            relation(&host, &launchd_service, "ssh.host-launchd-service"),
            relation(&host, &systemd_service, "ssh.host-systemd-service"),
        ],
        sensitive_field_policy: vec![
            "private keys, passphrases and SSH Agent material are never read".into(),
            "aliases, addresses, usernames, host key fingerprints and raw output are not logged".into(),
            "environment, history, arbitrary files, service definitions and logs are never requested".into(),
        ],
        rate_limit: RateLimitGuidance {
            default_max_concurrency: 1,
            requests_per_minute: None,
            respects_retry_after: false,
        },
        recommended_sync_interval_secs: 300,
        known_gaps: vec![
            "arbitrary commands, terminal access, sudo, port forwarding and automatic host key acceptance are unsupported".into(),
            "environment, history, arbitrary files, secrets and logs are unsupported; probes run as the configured non-root SSH user".into(),
            "Windows and non-systemd Linux service managers are unsupported".into(),
            "live SSH aliases have not been validated".into(),
            "targeted sync cannot provide authoritative missing evidence until coverage accepts provider locators".into(),
        ],
    }
}

fn kind(value: &str) -> ResourceKind {
    ResourceKind::new(value).expect("static resource kind")
}

fn resource(kind: ResourceKind, module: &str) -> ResourceCapability {
    ResourceCapability {
        kind,
        attribute_schema_version: SchemaVersion::new(1).expect("static schema version"),
        coverage: ConnectorCoverage {
            module: module.into(),
            level: ConnectorCoverageLevel::Supported,
            reason: None,
        },
    }
}

fn relation(
    source_kind: &ResourceKind,
    target_kind: &ResourceKind,
    module: &str,
) -> RelationCapability {
    RelationCapability {
        kind: RelationKind::new("ssh.contains").expect("static relation kind"),
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
    fn descriptor_is_supported_and_conformant() {
        let descriptor = ssh_descriptor();
        assert!(descriptor.validate().is_ok());
        assert!(check_descriptor(&descriptor).is_empty());
        assert!(descriptor.resources.iter().all(|capability| {
            capability.coverage.level == ConnectorCoverageLevel::Supported
                && capability.coverage.reason.is_none()
        }));
        let serialized = serde_json::to_string(&descriptor)
            .unwrap()
            .to_ascii_lowercase();
        assert!(!serialized.contains("run_command"));
        assert!(!serialized.contains("host key fingerprint:"));
    }
}
