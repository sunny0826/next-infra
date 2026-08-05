//! Shared, fail-closed contract for the installed Desktop Host and MCP Bridge.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

const DIRECTORY_MODE: u32 = 0o700;
const RECORD_MODE: u32 = 0o600;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntegrationPaths {
    pub root: PathBuf,
    pub state_dir: PathBuf,
    pub user_quit: PathBuf,
    pub integration_dir: PathBuf,
    pub mcp_dir: PathBuf,
    pub releases_dir: PathBuf,
    pub current_link: PathBuf,
    pub record: PathBuf,
    pub run_dir: PathBuf,
    pub launch_lock: PathBuf,
    pub stable_app: PathBuf,
    pub stable_bridge: PathBuf,
}

impl IntegrationPaths {
    pub fn from_home(home: &Path) -> Self {
        let root = home
            .join("Library")
            .join("Application Support")
            .join("Next Infra");
        let integration_dir = root.join("integration");
        let mcp_dir = integration_dir.join("mcp");
        let current_link = mcp_dir.join("current");
        Self {
            state_dir: root.join("state"),
            user_quit: root.join("state").join("user-quit-v1.json"),
            releases_dir: mcp_dir.join("releases"),
            record: mcp_dir.join("integration-v1.json"),
            run_dir: root.join("run"),
            launch_lock: root.join("run").join("mcp-launch-v1.lock"),
            stable_app: home.join("Applications").join("Next Infra.app"),
            stable_bridge: current_link.join("next-infra-mcp"),
            integration_dir,
            mcp_dir,
            current_link,
            root,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserQuitInspection {
    Clear,
    Suppressed,
}

#[derive(Debug, PartialEq, Eq)]
pub enum IntegrationError {
    Missing(&'static str),
    Invalid(&'static str),
    Io,
}

impl fmt::Display for IntegrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(label) => write!(formatter, "missing {label}"),
            Self::Invalid(label) => write!(formatter, "invalid {label}"),
            Self::Io => formatter.write_str("local integration state is unavailable"),
        }
    }
}

impl std::error::Error for IntegrationError {}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IntegrationRecord {
    pub schema_version: u32,
    pub release_id: String,
    pub stable_app_path: PathBuf,
    pub stable_bridge_path: PathBuf,
    pub bundle_id: String,
    pub team_id: Option<String>,
    pub app_designated_requirement: String,
    pub bridge_designated_requirement: String,
    pub protocol_major: u16,
    pub protocol_minor: u16,
    pub minimum_supported_minor: u16,
    pub host_supported_capabilities: Vec<String>,
    pub host_required_capabilities: Vec<String>,
    pub bridge_supported_capabilities: Vec<String>,
    pub bridge_required_capabilities: Vec<String>,
    pub allow_mcp_auto_launch: bool,
    pub installed_at: String,
    pub updated_at: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UserQuitMarker {
    schema_version: u32,
    user_quit: bool,
}

pub fn inspect_user_quit(paths: &IntegrationPaths) -> UserQuitInspection {
    let metadata = match fs::symlink_metadata(&paths.user_quit) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return UserQuitInspection::Clear;
        }
        Err(_) => return UserQuitInspection::Suppressed,
    };
    if validate_directory(&paths.state_dir).is_err()
        || validate_regular_file(&metadata, RECORD_MODE).is_err()
    {
        return UserQuitInspection::Suppressed;
    }
    match fs::read(&paths.user_quit)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<UserQuitMarker>(&bytes).ok())
    {
        Some(marker) if marker.schema_version == 1 && marker.user_quit => {
            UserQuitInspection::Suppressed
        }
        _ => UserQuitInspection::Suppressed,
    }
}

pub fn persist_user_quit(paths: &IntegrationPaths) -> Result<(), IntegrationError> {
    ensure_state_directory(paths)?;
    let temporary = paths
        .state_dir
        .join(format!("user-quit-v1.json.tmp-{}", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(RECORD_MODE)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&temporary)
        .map_err(|_| IntegrationError::Io)?;
    let result = (|| {
        file.write_all(br#"{"schema_version":1,"user_quit":true}"#)
            .map_err(|_| IntegrationError::Io)?;
        file.sync_all().map_err(|_| IntegrationError::Io)?;
        fs::rename(&temporary, &paths.user_quit).map_err(|_| IntegrationError::Io)?;
        sync_directory(&paths.state_dir)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub fn clear_user_quit(paths: &IntegrationPaths) -> Result<(), IntegrationError> {
    ensure_state_directory(paths)?;
    let metadata = match fs::symlink_metadata(&paths.user_quit) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(IntegrationError::Io),
    };
    validate_regular_file(&metadata, RECORD_MODE)?;
    fs::remove_file(&paths.user_quit).map_err(|_| IntegrationError::Io)?;
    if fs::symlink_metadata(&paths.user_quit).is_ok() {
        return Err(IntegrationError::Invalid("user quit marker replacement"));
    }
    sync_directory(&paths.state_dir)
}

pub fn validate_integration_record_for_bridge(
    paths: &IntegrationPaths,
    current_executable: &Path,
) -> Result<IntegrationRecord, IntegrationError> {
    let record = validate_common(paths)?;
    let resolved_bridge =
        fs::canonicalize(&paths.stable_bridge).map_err(|_| IntegrationError::Io)?;
    let resolved_current =
        fs::canonicalize(current_executable).map_err(|_| IntegrationError::Io)?;
    if resolved_bridge != resolved_current {
        return Err(IntegrationError::Invalid("running Bridge release"));
    }
    Ok(record)
}

pub fn validate_integration_record_for_host(
    paths: &IntegrationPaths,
    current_app_bundle: &Path,
) -> Result<IntegrationRecord, IntegrationError> {
    let record = validate_common(paths)?;
    let stable_app = fs::canonicalize(&paths.stable_app).map_err(|_| IntegrationError::Io)?;
    let current_app = fs::canonicalize(current_app_bundle).map_err(|_| IntegrationError::Io)?;
    if stable_app != current_app {
        return Err(IntegrationError::Invalid("running Desktop App"));
    }
    Ok(record)
}

pub fn authorize_mcp_host_launch(
    paths: &IntegrationPaths,
    current_app_bundle: &Path,
) -> Result<IntegrationRecord, IntegrationError> {
    if inspect_user_quit(paths) != UserQuitInspection::Clear {
        return Err(IntegrationError::Invalid("user quit marker"));
    }
    let record = validate_integration_record_for_host(paths, current_app_bundle)?;
    if !record.allow_mcp_auto_launch || inspect_user_quit(paths) != UserQuitInspection::Clear {
        return Err(IntegrationError::Invalid("MCP auto-launch authorization"));
    }
    Ok(record)
}

fn validate_common(paths: &IntegrationPaths) -> Result<IntegrationRecord, IntegrationError> {
    for directory in [
        &paths.root,
        &paths.integration_dir,
        &paths.mcp_dir,
        &paths.releases_dir,
        &paths.state_dir,
        &paths.run_dir,
    ] {
        validate_directory(directory)?;
    }
    let metadata = fs::symlink_metadata(&paths.record)
        .map_err(|error| map_missing(error, "integration record"))?;
    validate_regular_file(&metadata, RECORD_MODE)?;
    let record: IntegrationRecord =
        serde_json::from_slice(&fs::read(&paths.record).map_err(|_| IntegrationError::Io)?)
            .map_err(|_| IntegrationError::Invalid("integration record"))?;
    validate_record_contract(paths, &record)?;
    validate_current_release(paths, &record)?;
    validate_app_path(paths)?;
    Ok(record)
}

fn validate_record_contract(
    paths: &IntegrationPaths,
    record: &IntegrationRecord,
) -> Result<(), IntegrationError> {
    use next_infra_local_rpc::protocol::{
        Capability, MINIMUM_SUPPORTED_MINOR, PROTOCOL_MAJOR, PROTOCOL_MINOR,
    };
    if record.schema_version != 1
        || record.protocol_major != PROTOCOL_MAJOR
        || record.protocol_minor != PROTOCOL_MINOR
        || record.minimum_supported_minor != MINIMUM_SUPPORTED_MINOR
        || record.stable_app_path != paths.stable_app
        || record.stable_bridge_path != paths.stable_bridge
        || !is_single_component(&record.release_id)
        || record.bundle_id.trim().is_empty()
        || record.app_designated_requirement.trim().is_empty()
        || record.bridge_designated_requirement.trim().is_empty()
        || record.installed_at.trim().is_empty()
        || record.updated_at.trim().is_empty()
    {
        return Err(IntegrationError::Invalid("integration contract"));
    }
    let expected = Capability::ALL.map(Capability::as_str);
    validate_capabilities(&record.host_supported_capabilities, &expected)?;
    validate_capabilities(&record.bridge_required_capabilities, &expected)?;
    validate_capabilities(&record.host_required_capabilities, &[])?;
    validate_capabilities(&record.bridge_supported_capabilities, &[])?;
    if record.allow_mcp_auto_launch && record.team_id.as_deref().is_none_or(str::is_empty) {
        return Err(IntegrationError::Invalid("auto-launch signing identity"));
    }
    Ok(())
}

fn validate_capabilities(actual: &[String], expected: &[&str]) -> Result<(), IntegrationError> {
    if actual
        .iter()
        .map(String::as_str)
        .ne(expected.iter().copied())
    {
        return Err(IntegrationError::Invalid("capability set"));
    }
    Ok(())
}

fn validate_current_release(
    paths: &IntegrationPaths,
    record: &IntegrationRecord,
) -> Result<(), IntegrationError> {
    let metadata = fs::symlink_metadata(&paths.current_link)
        .map_err(|error| map_missing(error, "current release link"))?;
    if !metadata.file_type().is_symlink() || metadata.uid() != current_euid() {
        return Err(IntegrationError::Invalid("current release link"));
    }
    let target = fs::read_link(&paths.current_link).map_err(|_| IntegrationError::Io)?;
    let expected = Path::new("releases").join(&record.release_id);
    if target != expected || target.is_absolute() || has_non_normal_component(&target) {
        return Err(IntegrationError::Invalid("current release target"));
    }
    validate_directory(&paths.releases_dir.join(&record.release_id))?;
    let bridge = fs::symlink_metadata(&paths.stable_bridge)
        .map_err(|error| map_missing(error, "stable Bridge"))?;
    validate_executable(&bridge)
}

fn validate_app_path(paths: &IntegrationPaths) -> Result<(), IntegrationError> {
    let parent = paths
        .stable_app
        .parent()
        .ok_or(IntegrationError::Invalid("Applications directory"))?;
    for (path, label) in [
        (parent, "Applications directory"),
        (paths.stable_app.as_path(), "stable App"),
    ] {
        let metadata = fs::symlink_metadata(path).map_err(|error| map_missing(error, label))?;
        if metadata.file_type().is_symlink()
            || !metadata.is_dir()
            || metadata.uid() != current_euid()
        {
            return Err(IntegrationError::Invalid(label));
        }
    }
    Ok(())
}

fn ensure_state_directory(paths: &IntegrationPaths) -> Result<(), IntegrationError> {
    ensure_owner_directory(&paths.root)?;
    ensure_owner_directory(&paths.state_dir)
}

fn ensure_owner_directory(path: &Path) -> Result<(), IntegrationError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink()
                || !metadata.is_dir()
                || metadata.uid() != current_euid()
            {
                return Err(IntegrationError::Invalid("integration directory"));
            }
            if metadata.permissions().mode() & 0o7777 != DIRECTORY_MODE {
                fs::set_permissions(path, fs::Permissions::from_mode(DIRECTORY_MODE))
                    .map_err(|_| IntegrationError::Io)?;
            }
            validate_directory(path)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(path).map_err(|_| IntegrationError::Io)?;
            fs::set_permissions(path, fs::Permissions::from_mode(DIRECTORY_MODE))
                .map_err(|_| IntegrationError::Io)?;
            validate_directory(path)
        }
        Err(_) => Err(IntegrationError::Io),
    }
}

fn validate_directory(path: &Path) -> Result<(), IntegrationError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| map_missing(error, "integration directory"))?;
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || metadata.uid() != current_euid()
        || metadata.permissions().mode() & 0o7777 != DIRECTORY_MODE
    {
        return Err(IntegrationError::Invalid("integration directory"));
    }
    Ok(())
}

fn validate_regular_file(metadata: &fs::Metadata, mode: u32) -> Result<(), IntegrationError> {
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.uid() != current_euid()
        || metadata.permissions().mode() & 0o7777 != mode
    {
        return Err(IntegrationError::Invalid("integration file"));
    }
    Ok(())
}

fn validate_executable(metadata: &fs::Metadata) -> Result<(), IntegrationError> {
    let mode = metadata.permissions().mode() & 0o7777;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.uid() != current_euid()
        || mode & 0o022 != 0
        || mode & 0o100 == 0
    {
        return Err(IntegrationError::Invalid("Bridge executable"));
    }
    Ok(())
}

fn sync_directory(path: &Path) -> Result<(), IntegrationError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| IntegrationError::Io)
}

fn map_missing(error: std::io::Error, label: &'static str) -> IntegrationError {
    if error.kind() == std::io::ErrorKind::NotFound {
        IntegrationError::Missing(label)
    } else {
        IntegrationError::Io
    }
}

fn is_single_component(value: &str) -> bool {
    !value.is_empty()
        && Path::new(value).components().count() == 1
        && matches!(
            Path::new(value).components().next(),
            Some(Component::Normal(_))
        )
}

fn has_non_normal_component(path: &Path) -> bool {
    path.components()
        .any(|component| !matches!(component, Component::Normal(_)))
}

fn current_euid() -> u32 {
    unsafe { libc::geteuid() as u32 }
}
