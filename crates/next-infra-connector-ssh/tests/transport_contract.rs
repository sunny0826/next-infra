use next_infra_connector_contract_tests::check_descriptor;
use next_infra_connector_ssh::{
    HostAlias, HostIdentity, ProbeId, ProbePlatform, SshConnectionConfigV1, probe_metadata,
    probe_registry, ssh_descriptor,
};
use serde_json::json;

const HOST_IDENTITY: &str = "9f7fd5e6-3bc8-4daa-ae6b-9dfdffb54743";

#[test]
fn public_contract_exposes_identity_and_metadata_without_commands() {
    let identity = HostIdentity::parse(HOST_IDENTITY).unwrap();
    assert_eq!(
        identity.external_id().as_str(),
        "ssh-host:v1:9f7fd5e6-3bc8-4daa-ae6b-9dfdffb54743"
    );
    assert!(HostAlias::parse("fixture-host").is_ok());

    let metadata = probe_metadata(ProbeId::MacosLaunchdServicesV1);
    assert_eq!(metadata.platform, ProbePlatform::Macos);
    assert_eq!(metadata.timeout_secs, 20);
    let serialized = serde_json::to_string(&probe_registry()).unwrap();
    for forbidden in ["uname", "df -Pk", "launchctl", "systemctl"] {
        assert!(!serialized.contains(forbidden));
    }

    let descriptor = ssh_descriptor();
    assert!(descriptor.validate().is_ok());
    assert!(check_descriptor(&descriptor).is_empty());
}

#[test]
fn public_config_parser_rejects_dynamic_transport_fields_without_echoing_values() {
    for field in [
        "hostname",
        "ip",
        "port",
        "username",
        "command",
        "args",
        "ProxyCommand",
    ] {
        let mut value = json!({
            "host_identity": HOST_IDENTITY,
            "host_alias": "fixture-host",
            "probe_profile": "baseline-v1"
        });
        value
            .as_object_mut()
            .unwrap()
            .insert(field.into(), json!("secret-output-sentinel"));
        let failure = SshConnectionConfigV1::from_json(value).unwrap_err();
        assert!(!format!("{failure:?}").contains("secret-output-sentinel"));
    }
}
