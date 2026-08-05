#![cfg(unix)]

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;

use next_infra_host_integration::{
    IntegrationPaths, IntegrationRecord, UserQuitInspection, authorize_mcp_host_launch,
    clear_user_quit, inspect_user_quit, persist_user_quit, validate_integration_record_for_host,
};
use next_infra_local_rpc::protocol::{
    Capability, MINIMUM_SUPPORTED_MINOR, PROTOCOL_MAJOR, PROTOCOL_MINOR,
};
use tempfile::{Builder, TempDir};

#[test]
fn marker_persist_and_clear_use_owner_only_state() {
    let temp = Builder::new()
        .prefix("ni-host-state")
        .tempdir_in("/tmp")
        .unwrap();
    let paths = IntegrationPaths::from_home(temp.path());

    persist_user_quit(&paths).unwrap();
    assert_eq!(
        fs::symlink_metadata(&paths.state_dir)
            .unwrap()
            .permissions()
            .mode()
            & 0o7777,
        0o700
    );
    assert_eq!(
        fs::symlink_metadata(&paths.user_quit)
            .unwrap()
            .permissions()
            .mode()
            & 0o7777,
        0o600
    );
    assert_eq!(inspect_user_quit(&paths), UserQuitInspection::Suppressed);

    clear_user_quit(&paths).unwrap();
    assert_eq!(inspect_user_quit(&paths), UserQuitInspection::Clear);
    clear_user_quit(&paths).unwrap();
}

#[test]
fn marker_clear_refuses_symlink_and_wrong_mode() {
    let temp = Builder::new()
        .prefix("ni-host-clear")
        .tempdir_in("/tmp")
        .unwrap();
    let paths = IntegrationPaths::from_home(temp.path());
    persist_user_quit(&paths).unwrap();
    fs::set_permissions(&paths.user_quit, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(clear_user_quit(&paths).is_err());
    assert!(paths.user_quit.exists());

    fs::remove_file(&paths.user_quit).unwrap();
    let target = temp.path().join("target");
    write_mode(&target, b"fixture", 0o600);
    symlink(&target, &paths.user_quit).unwrap();
    assert!(clear_user_quit(&paths).is_err());
    assert!(target.exists());
}

#[test]
fn host_record_requires_the_running_stable_app() {
    let fixture = Fixture::new();
    assert!(
        validate_integration_record_for_host(&fixture.paths, &fixture.paths.stable_app).is_ok()
    );
    let other_app = fixture.temp.path().join("Other.app");
    create_dir_mode(&other_app);
    assert!(validate_integration_record_for_host(&fixture.paths, &other_app).is_err());
}

#[test]
fn mcp_host_authorization_never_clears_marker_and_requires_enabled_record() {
    let fixture = Fixture::new();
    assert!(authorize_mcp_host_launch(&fixture.paths, &fixture.paths.stable_app).is_ok());

    persist_user_quit(&fixture.paths).unwrap();
    assert!(authorize_mcp_host_launch(&fixture.paths, &fixture.paths.stable_app).is_err());
    assert_eq!(
        inspect_user_quit(&fixture.paths),
        UserQuitInspection::Suppressed
    );
    clear_user_quit(&fixture.paths).unwrap();

    let mut record = fixture.record.clone();
    record.allow_mcp_auto_launch = false;
    fixture.write_record(&record);
    assert!(authorize_mcp_host_launch(&fixture.paths, &fixture.paths.stable_app).is_err());
}

struct Fixture {
    temp: TempDir,
    paths: IntegrationPaths,
    record: IntegrationRecord,
}

impl Fixture {
    fn new() -> Self {
        let temp = Builder::new()
            .prefix("ni-host-record")
            .tempdir_in("/tmp")
            .unwrap();
        let home = temp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        let paths = IntegrationPaths::from_home(&home);
        create_dir_mode(&paths.root);
        for directory in [
            &paths.integration_dir,
            &paths.mcp_dir,
            &paths.releases_dir,
            &paths.state_dir,
            &paths.run_dir,
            &home.join("Applications"),
            &paths.releases_dir.join("0.1.0"),
            &paths.stable_app,
        ] {
            create_dir_mode(directory);
        }
        let bridge = paths.releases_dir.join("0.1.0").join("next-infra-mcp");
        write_mode(&bridge, b"fixture", 0o700);
        symlink("releases/0.1.0", &paths.current_link).unwrap();
        let capabilities: Vec<String> = Capability::ALL
            .map(Capability::as_str)
            .into_iter()
            .map(str::to_owned)
            .collect();
        let record = IntegrationRecord {
            schema_version: 1,
            release_id: "0.1.0".into(),
            stable_app_path: paths.stable_app.clone(),
            stable_bridge_path: paths.stable_bridge.clone(),
            bundle_id: "dev.guoxudong.next-infra".into(),
            team_id: Some("TEAMID1234".into()),
            app_designated_requirement: "identifier dev.guoxudong.next-infra".into(),
            bridge_designated_requirement: "identifier next-infra-mcp".into(),
            protocol_major: PROTOCOL_MAJOR,
            protocol_minor: PROTOCOL_MINOR,
            minimum_supported_minor: MINIMUM_SUPPORTED_MINOR,
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

fn create_dir_mode(path: &Path) {
    fs::create_dir_all(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

fn write_mode(path: &Path, bytes: &[u8], mode: u32) {
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}
