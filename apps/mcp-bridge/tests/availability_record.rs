#![cfg(unix)]

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use next_infra_mcp_bridge::availability::{
    IntegrationPaths, IntegrationRecord, UserQuitInspection, inspect_user_quit,
    validate_integration_record,
};
use tempfile::{Builder, TempDir};

#[test]
fn valid_record_and_release_paths_are_accepted() {
    let fixture = Fixture::new();
    let record = validate_integration_record(&fixture.paths, &fixture.bridge).unwrap();
    assert!(record.allow_mcp_auto_launch);
    assert_eq!(record.release_id, "0.1.0");
}

#[test]
fn user_quit_is_fail_closed_for_every_existing_invalid_form() {
    let fixture = Fixture::new();
    assert_eq!(inspect_user_quit(&fixture.paths), UserQuitInspection::Clear);

    write_mode(
        &fixture.paths.user_quit,
        br#"{"schema_version":1,"user_quit":true}"#,
        0o600,
    );
    assert_eq!(
        inspect_user_quit(&fixture.paths),
        UserQuitInspection::Suppressed
    );

    fs::write(&fixture.paths.user_quit, b"not-json").unwrap();
    assert_eq!(
        inspect_user_quit(&fixture.paths),
        UserQuitInspection::Suppressed
    );

    fs::write(
        &fixture.paths.user_quit,
        br#"{"schema_version":1,"user_quit":false}"#,
    )
    .unwrap();
    assert_eq!(
        inspect_user_quit(&fixture.paths),
        UserQuitInspection::Suppressed
    );

    fs::set_permissions(&fixture.paths.user_quit, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(
        inspect_user_quit(&fixture.paths),
        UserQuitInspection::Suppressed
    );

    fs::remove_file(&fixture.paths.user_quit).unwrap();
    let target = fixture.temp.path().join("marker-target");
    write_mode(&target, br#"{"schema_version":1,"user_quit":true}"#, 0o600);
    symlink(target, &fixture.paths.user_quit).unwrap();
    assert_eq!(
        inspect_user_quit(&fixture.paths),
        UserQuitInspection::Suppressed
    );
}

#[test]
fn record_contract_path_and_permission_drift_fail_closed() {
    let fixture = Fixture::new();
    let mut record = fixture.record.clone();

    record.team_id = None;
    fixture.write_record(&record);
    assert!(validate_integration_record(&fixture.paths, &fixture.bridge).is_err());

    record = fixture.record.clone();
    record.host_supported_capabilities.reverse();
    fixture.write_record(&record);
    assert!(validate_integration_record(&fixture.paths, &fixture.bridge).is_err());

    fixture.write_record(&fixture.record);
    fs::set_permissions(&fixture.paths.record, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(validate_integration_record(&fixture.paths, &fixture.bridge).is_err());
    fs::set_permissions(&fixture.paths.record, fs::Permissions::from_mode(0o600)).unwrap();

    fs::remove_file(&fixture.paths.current_link).unwrap();
    symlink("releases/other", &fixture.paths.current_link).unwrap();
    assert!(validate_integration_record(&fixture.paths, &fixture.bridge).is_err());
}

#[test]
fn unknown_record_fields_and_running_artifact_mismatch_are_rejected() {
    let fixture = Fixture::new();
    let mut value = serde_json::to_value(&fixture.record).unwrap();
    value["unexpected"] = serde_json::json!(true);
    write_mode(
        &fixture.paths.record,
        serde_json::to_vec(&value).unwrap().as_slice(),
        0o600,
    );
    assert!(validate_integration_record(&fixture.paths, &fixture.bridge).is_err());

    fixture.write_record(&fixture.record);
    let other = fixture.temp.path().join("other-bridge");
    write_mode(&other, b"fixture", 0o700);
    assert!(validate_integration_record(&fixture.paths, &other).is_err());
}

struct Fixture {
    temp: TempDir,
    paths: IntegrationPaths,
    bridge: PathBuf,
    record: IntegrationRecord,
}

impl Fixture {
    fn new() -> Self {
        let temp = Builder::new()
            .prefix("ni-avail")
            .tempdir_in("/tmp")
            .unwrap();
        let home = temp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        let paths = IntegrationPaths::from_home(&home);
        create_dir_mode(&paths.root, 0o700);
        for directory in [
            &paths.integration_dir,
            &paths.mcp_dir,
            &paths.releases_dir,
            &paths.state_dir,
            &paths.run_dir,
            &home.join("Applications"),
            &paths.releases_dir.join("0.1.0"),
        ] {
            create_dir_mode(directory, 0o700);
        }
        create_dir_mode(&paths.stable_app, 0o700);

        let bridge = paths.releases_dir.join("0.1.0").join("next-infra-mcp");
        write_mode(&bridge, b"fixture bridge", 0o700);
        symlink("releases/0.1.0", &paths.current_link).unwrap();

        let capabilities = next_infra_mcp::local_rpc_capability_names()
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let record = IntegrationRecord {
            schema_version: 1,
            release_id: "0.1.0".into(),
            stable_app_path: paths.stable_app.clone(),
            stable_bridge_path: paths.stable_bridge.clone(),
            bundle_id: "dev.guoxudong.next-infra".into(),
            team_id: Some("TEAMID1234".into()),
            app_designated_requirement: "identifier dev.guoxudong.next-infra".into(),
            bridge_designated_requirement: "identifier next-infra-mcp".into(),
            protocol_major: next_infra_mcp::LOCAL_RPC_PROTOCOL_MAJOR,
            protocol_minor: next_infra_mcp::LOCAL_RPC_PROTOCOL_MINOR,
            minimum_supported_minor: next_infra_mcp::LOCAL_RPC_MINIMUM_SUPPORTED_MINOR,
            host_supported_capabilities: capabilities.clone(),
            host_required_capabilities: vec![],
            bridge_supported_capabilities: vec![],
            bridge_required_capabilities: capabilities,
            allow_mcp_auto_launch: true,
            installed_at: "2026-08-04T00:00:00Z".into(),
            updated_at: "2026-08-04T00:00:00Z".into(),
        };
        let fixture = Self {
            temp,
            paths,
            bridge,
            record,
        };
        fixture.write_record(&fixture.record);
        fixture
    }

    fn write_record(&self, record: &IntegrationRecord) {
        write_mode(
            &self.paths.record,
            serde_json::to_vec(record).unwrap().as_slice(),
            0o600,
        );
    }
}

fn create_dir_mode(path: &Path, mode: u32) {
    fs::create_dir_all(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

fn write_mode(path: &Path, bytes: &[u8], mode: u32) {
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}
