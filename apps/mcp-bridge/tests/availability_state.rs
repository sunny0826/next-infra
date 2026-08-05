#![cfg(unix)]

use std::cell::Cell;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

use next_infra_mcp::McpBridgeError;
use next_infra_mcp_bridge::availability::{
    AvailabilityActionError, AvailabilityPolicy, HostConnector, HostLauncher, IntegrationPaths,
    IntegrationRecord, MacOpenLauncher, MonotonicClock, SignatureVerifier, VerifiedArtifacts,
    ensure_host, open_command,
};
use tempfile::{Builder, TempDir};

#[test]
fn running_host_bypasses_all_installation_state() {
    let temp = Builder::new().prefix("ni-fast").tempdir_in("/tmp").unwrap();
    let paths = IntegrationPaths::from_home(temp.path());
    let connector = FakeConnector::new(1, None);
    let verifier = FakeVerifier::new(true);
    let launcher = FakeLauncher::new(true);
    let clock = FakeClock::default();

    assert_eq!(
        ensure_host(
            &paths,
            &temp.path().join("missing"),
            &connector,
            &verifier,
            &launcher,
            &clock,
            policy(),
        )
        .unwrap(),
        1
    );
    assert_eq!(connector.calls.get(), 1);
    assert_eq!(verifier.calls.get(), 0);
    assert_eq!(launcher.calls.get(), 0);
}

#[test]
fn user_quit_and_disabled_record_suppress_without_launching() {
    let fixture = Fixture::new();
    fixture.write_marker();
    let connector = FakeConnector::new(usize::MAX, None);
    let verifier = FakeVerifier::new(true);
    let launcher = FakeLauncher::new(true);
    assert!(
        ensure_host(
            &fixture.paths,
            &fixture.bridge,
            &connector,
            &verifier,
            &launcher,
            &FakeClock::default(),
            policy(),
        )
        .is_err()
    );
    assert_eq!(verifier.calls.get(), 0);
    assert_eq!(launcher.calls.get(), 0);

    fs::remove_file(&fixture.paths.user_quit).unwrap();
    let mut record = fixture.record.clone();
    record.allow_mcp_auto_launch = false;
    fixture.write_record(&record);
    assert!(
        ensure_host(
            &fixture.paths,
            &fixture.bridge,
            &connector,
            &verifier,
            &launcher,
            &FakeClock::default(),
            policy(),
        )
        .is_err()
    );
    assert_eq!(verifier.calls.get(), 0);
    assert_eq!(launcher.calls.get(), 0);
}

#[test]
fn signature_failure_never_calls_launcher() {
    let fixture = Fixture::new();
    let verifier = FakeVerifier::new(false);
    let launcher = FakeLauncher::new(true);
    assert!(
        ensure_host(
            &fixture.paths,
            &fixture.bridge,
            &FakeConnector::new(usize::MAX, None),
            &verifier,
            &launcher,
            &FakeClock::default(),
            policy(),
        )
        .is_err()
    );
    assert_eq!(verifier.calls.get(), 1);
    assert_eq!(launcher.calls.get(), 0);
}

#[test]
fn artifact_replacement_after_signature_verification_is_rejected() {
    let fixture = Fixture::new();
    let launcher = FakeLauncher::new(true);
    assert!(
        ensure_host(
            &fixture.paths,
            &fixture.bridge,
            &FakeConnector::new(usize::MAX, None),
            &ReplacingVerifier,
            &launcher,
            &FakeClock::default(),
            policy(),
        )
        .is_err()
    );
    assert_eq!(launcher.calls.get(), 0);
}

#[test]
fn valid_fixture_launches_once_and_waits_for_handshake() {
    let fixture = Fixture::new();
    let connector = FakeConnector::new(3, None);
    let verifier = FakeVerifier::new(true);
    let launcher = FakeLauncher::new(true);
    let clock = FakeClock::default();
    let client = ensure_host(
        &fixture.paths,
        &fixture.bridge,
        &connector,
        &verifier,
        &launcher,
        &clock,
        policy(),
    )
    .unwrap();
    assert_eq!(client, 3);
    assert_eq!(verifier.calls.get(), 1);
    assert_eq!(launcher.calls.get(), 1);
    assert_eq!(connector.calls.get(), 3);
    assert!(clock.now.get() <= policy().timeout);
}

#[test]
fn follower_waits_without_reverifying_or_launching() {
    let fixture = Fixture::new();
    let connector = FakeConnector::new(2, None);
    let verifier = FakeVerifier::new(true);
    let launcher = FakeLauncher::follower();
    assert_eq!(
        ensure_host(
            &fixture.paths,
            &fixture.bridge,
            &connector,
            &verifier,
            &launcher,
            &FakeClock::default(),
            policy(),
        )
        .unwrap(),
        2
    );
    assert_eq!(verifier.calls.get(), 0);
    assert_eq!(launcher.calls.get(), 0);
}

#[test]
fn production_launch_lock_is_exclusive_across_processes() {
    let fixture = Fixture::new();
    let ready = fixture._temp.path().join("launch-lock-ready");
    let release = fixture._temp.path().join("launch-lock-release");
    let mut child = Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("launch_lock_child_helper")
        .arg("--nocapture")
        .env("NEXT_INFRA_LOCK_HELPER_HOME", &fixture.home)
        .env("NEXT_INFRA_LOCK_HELPER_READY", &ready)
        .env("NEXT_INFRA_LOCK_HELPER_RELEASE", &release)
        .spawn()
        .unwrap();
    for _ in 0..100 {
        if ready.exists() {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert!(ready.exists());

    let launcher = MacOpenLauncher::default();
    assert!(launcher.coordinate(&fixture.paths).unwrap().is_none());
    fs::write(&release, b"release").unwrap();
    assert!(child.wait().unwrap().success());
    assert!(launcher.coordinate(&fixture.paths).unwrap().is_some());
}

#[test]
fn launch_lock_child_helper() {
    let Some(home) = std::env::var_os("NEXT_INFRA_LOCK_HELPER_HOME") else {
        return;
    };
    let ready = PathBuf::from(std::env::var_os("NEXT_INFRA_LOCK_HELPER_READY").unwrap());
    let release = PathBuf::from(std::env::var_os("NEXT_INFRA_LOCK_HELPER_RELEASE").unwrap());
    let paths = IntegrationPaths::from_home(Path::new(&home));
    let launcher = MacOpenLauncher::default();
    let _guard = launcher.coordinate(&paths).unwrap().unwrap();
    fs::write(&ready, b"ready").unwrap();
    while !release.exists() {
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn timeout_is_capped_at_ten_seconds() {
    let fixture = Fixture::new();
    let clock = FakeClock::default();
    let long_policy = AvailabilityPolicy {
        timeout: Duration::from_secs(60),
        initial_delay: Duration::from_millis(100),
        maximum_delay: Duration::from_millis(100),
    };
    assert!(
        ensure_host(
            &fixture.paths,
            &fixture.bridge,
            &FakeConnector::new(usize::MAX, None),
            &FakeVerifier::new(true),
            &FakeLauncher::new(true),
            &clock,
            long_policy,
        )
        .is_err()
    );
    assert_eq!(clock.now.get(), Duration::from_secs(10));
}

#[test]
fn zero_timeout_or_backoff_is_rejected_before_connecting() {
    let temp = Builder::new().prefix("ni-zero").tempdir_in("/tmp").unwrap();
    let paths = IntegrationPaths::from_home(temp.path());
    for invalid in [
        AvailabilityPolicy {
            timeout: Duration::ZERO,
            ..policy()
        },
        AvailabilityPolicy {
            initial_delay: Duration::ZERO,
            ..policy()
        },
        AvailabilityPolicy {
            maximum_delay: Duration::ZERO,
            ..policy()
        },
    ] {
        let connector = FakeConnector::new(1, None);
        assert!(
            ensure_host(
                &paths,
                &temp.path().join("missing"),
                &connector,
                &FakeVerifier::new(true),
                &FakeLauncher::new(true),
                &FakeClock::default(),
                invalid,
            )
            .is_err()
        );
        assert_eq!(connector.calls.get(), 0);
    }
}

#[test]
fn successful_connect_with_new_user_quit_is_suppressed() {
    let fixture = Fixture::new();
    let connector = FakeConnector::new(2, Some(fixture.paths.user_quit.clone()));
    assert!(
        ensure_host(
            &fixture.paths,
            &fixture.bridge,
            &connector,
            &FakeVerifier::new(true),
            &FakeLauncher::new(true),
            &FakeClock::default(),
            policy(),
        )
        .is_err()
    );
    assert_eq!(connector.calls.get(), 2);
}

#[test]
fn timeout_and_wait_time_user_quit_never_relaunch() {
    let fixture = Fixture::new();
    let launcher = FakeLauncher::new(true);
    let clock = FakeClock::default();
    assert!(
        ensure_host(
            &fixture.paths,
            &fixture.bridge,
            &FakeConnector::new(usize::MAX, None),
            &FakeVerifier::new(true),
            &launcher,
            &clock,
            policy(),
        )
        .is_err()
    );
    assert_eq!(launcher.calls.get(), 1);
    assert_eq!(clock.now.get(), policy().timeout);

    let fixture = Fixture::new();
    let connector = FakeConnector::new(usize::MAX, Some(fixture.paths.user_quit.clone()));
    let launcher = FakeLauncher::new(true);
    assert!(
        ensure_host(
            &fixture.paths,
            &fixture.bridge,
            &connector,
            &FakeVerifier::new(true),
            &launcher,
            &FakeClock::default(),
            policy(),
        )
        .is_err()
    );
    assert_eq!(launcher.calls.get(), 1);
    assert_eq!(connector.calls.get(), 2);
}

#[test]
fn production_open_command_is_fixed_and_shell_free() {
    let app = Path::new("/Users/fixture/Applications/Next Infra.app");
    let (program, arguments) = open_command(app);
    assert_eq!(program, Path::new("/usr/bin/open"));
    assert_eq!(
        arguments,
        [
            "-g",
            "/Users/fixture/Applications/Next Infra.app",
            "--args",
            "--background",
            "--launch-source=mcp",
        ]
        .map(std::ffi::OsString::from)
    );
}

fn policy() -> AvailabilityPolicy {
    AvailabilityPolicy {
        timeout: Duration::from_millis(300),
        initial_delay: Duration::from_millis(100),
        maximum_delay: Duration::from_millis(100),
    }
}

struct FakeConnector {
    calls: Cell<usize>,
    success_on: usize,
    marker_on_second_call: Option<PathBuf>,
}

impl FakeConnector {
    fn new(success_on: usize, marker_on_second_call: Option<PathBuf>) -> Self {
        Self {
            calls: Cell::new(0),
            success_on,
            marker_on_second_call,
        }
    }
}

impl HostConnector for FakeConnector {
    type Client = usize;

    fn connect(&self, _timeout: Duration) -> Result<Self::Client, McpBridgeError> {
        let call = self.calls.get() + 1;
        self.calls.set(call);
        if call == 2
            && let Some(marker) = &self.marker_on_second_call
        {
            write_mode(marker, br#"{"schema_version":1,"user_quit":true}"#, 0o600);
        }
        if call >= self.success_on {
            Ok(call)
        } else {
            Err(McpBridgeError::new("host_unavailable", "fixture", true))
        }
    }
}

struct FakeVerifier {
    calls: Cell<usize>,
    succeeds: bool,
}

struct ReplacingVerifier;

impl SignatureVerifier for ReplacingVerifier {
    fn verify(
        &self,
        paths: &IntegrationPaths,
        _record: &IntegrationRecord,
        current_executable: &Path,
    ) -> Result<VerifiedArtifacts, AvailabilityActionError> {
        let verified = VerifiedArtifacts::capture(paths, current_executable)?;
        fs::remove_dir(&paths.stable_app).unwrap();
        create_dir_mode(&paths.stable_app);
        Ok(verified)
    }
}

impl FakeVerifier {
    fn new(succeeds: bool) -> Self {
        Self {
            calls: Cell::new(0),
            succeeds,
        }
    }
}

impl SignatureVerifier for FakeVerifier {
    fn verify(
        &self,
        paths: &IntegrationPaths,
        _record: &IntegrationRecord,
        current_executable: &Path,
    ) -> Result<VerifiedArtifacts, AvailabilityActionError> {
        self.calls.set(self.calls.get() + 1);
        if !self.succeeds {
            return Err(AvailabilityActionError);
        }
        VerifiedArtifacts::capture(paths, current_executable)
    }
}

struct FakeLauncher {
    calls: Cell<usize>,
    succeeds: bool,
    leader: bool,
}

impl FakeLauncher {
    fn new(succeeds: bool) -> Self {
        Self {
            calls: Cell::new(0),
            succeeds,
            leader: true,
        }
    }

    fn follower() -> Self {
        Self {
            calls: Cell::new(0),
            succeeds: true,
            leader: false,
        }
    }
}

impl HostLauncher for FakeLauncher {
    type Guard = ();

    fn coordinate(
        &self,
        _paths: &IntegrationPaths,
    ) -> Result<Option<Self::Guard>, AvailabilityActionError> {
        Ok(self.leader.then_some(()))
    }

    fn launch(&self, _artifacts: &VerifiedArtifacts) -> Result<(), AvailabilityActionError> {
        self.calls.set(self.calls.get() + 1);
        self.succeeds.then_some(()).ok_or(AvailabilityActionError)
    }
}

#[derive(Default)]
struct FakeClock {
    now: Cell<Duration>,
}

impl MonotonicClock for FakeClock {
    fn now(&self) -> Duration {
        self.now.get()
    }

    fn sleep(&self, duration: Duration) {
        self.now.set(self.now.get() + duration);
    }
}

struct Fixture {
    _temp: TempDir,
    home: PathBuf,
    paths: IntegrationPaths,
    bridge: PathBuf,
    record: IntegrationRecord,
}

impl Fixture {
    fn new() -> Self {
        let temp = Builder::new()
            .prefix("ni-state")
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
        ] {
            create_dir_mode(directory);
        }
        create_dir_mode(&paths.stable_app);
        let bridge = paths.releases_dir.join("0.1.0").join("next-infra-mcp");
        write_mode(&bridge, b"fixture", 0o700);
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
            _temp: temp,
            home,
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

    fn write_marker(&self) {
        write_mode(
            &self.paths.user_quit,
            br#"{"schema_version":1,"user_quit":true}"#,
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
