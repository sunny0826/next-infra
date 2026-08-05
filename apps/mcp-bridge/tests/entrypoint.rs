#![cfg(unix)]

use std::process::{Command, Stdio};

use next_infra_host_integration::{
    IntegrationPaths, UserQuitInspection, inspect_user_quit, persist_user_quit,
};
use tempfile::Builder;

#[test]
fn missing_home_fails_closed_without_stdio_payload() {
    let output = Command::new(env!("CARGO_BIN_EXE_next-infra-mcp"))
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.stderr,
        b"next-infra-mcp: host_unavailable: The user home directory is unavailable.\n"
    );
}

#[test]
fn missing_host_returns_safe_error_without_creating_local_state() {
    let home = Builder::new()
        .prefix("ni-entrypoint")
        .tempdir_in("/tmp")
        .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_next-infra-mcp"))
        .env_clear()
        .env("HOME", home.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(
        stderr,
        "next-infra-mcp: host_unavailable: Next Infra is unavailable. Start it interactively or review MCP integration settings.\n"
    );
    assert!(!home.path().join("Library").exists());
}

#[test]
fn new_bridge_after_user_quit_is_suppressed_without_launch_attempt() {
    let home = Builder::new()
        .prefix("ni-entrypoint-quit")
        .tempdir_in("/tmp")
        .unwrap();
    let paths = IntegrationPaths::from_home(home.path());
    persist_user_quit(&paths).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_next-infra-mcp"))
        .env_clear()
        .env("HOME", home.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert_eq!(inspect_user_quit(&paths), UserQuitInspection::Suppressed);
    assert!(!paths.launch_lock.exists());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "next-infra-mcp: host_unavailable: Next Infra is unavailable. Start it interactively or review MCP integration settings.\n"
    );
}
