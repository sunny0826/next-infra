use std::fmt;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
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

#[derive(Debug)]
pub enum AvailabilityError {
    Missing(&'static str),
    Invalid(&'static str),
    Io,
}

impl fmt::Display for AvailabilityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(label) => write!(formatter, "missing {label}"),
            Self::Invalid(label) => write!(formatter, "invalid {label}"),
            Self::Io => formatter.write_str("local integration state is unavailable"),
        }
    }
}

impl std::error::Error for AvailabilityError {}

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
        || validate_regular_file(&paths.user_quit, &metadata, RECORD_MODE).is_err()
    {
        return UserQuitInspection::Suppressed;
    }
    let bytes = match fs::read(&paths.user_quit) {
        Ok(bytes) => bytes,
        Err(_) => return UserQuitInspection::Suppressed,
    };
    match serde_json::from_slice::<UserQuitMarker>(&bytes) {
        Ok(marker) if marker.schema_version == 1 && marker.user_quit => {
            UserQuitInspection::Suppressed
        }
        _ => UserQuitInspection::Suppressed,
    }
}

pub fn validate_integration_record(
    paths: &IntegrationPaths,
    current_executable: &Path,
) -> Result<IntegrationRecord, AvailabilityError> {
    for directory in [
        &paths.integration_dir,
        &paths.mcp_dir,
        &paths.releases_dir,
        &paths.state_dir,
        &paths.run_dir,
    ] {
        validate_directory(directory)?;
    }

    let record_metadata = fs::symlink_metadata(&paths.record)
        .map_err(|error| map_missing(error, "integration record"))?;
    validate_regular_file(&paths.record, &record_metadata, RECORD_MODE)?;
    let record: IntegrationRecord =
        serde_json::from_slice(&fs::read(&paths.record).map_err(|_| AvailabilityError::Io)?)
            .map_err(|_| AvailabilityError::Invalid("integration record"))?;

    validate_record_contract(paths, &record)?;
    validate_current_release(paths, &record, current_executable)?;
    validate_app_path(paths)?;
    Ok(record)
}

fn validate_record_contract(
    paths: &IntegrationPaths,
    record: &IntegrationRecord,
) -> Result<(), AvailabilityError> {
    if record.schema_version != 1
        || record.protocol_major != next_infra_mcp::LOCAL_RPC_PROTOCOL_MAJOR
        || record.protocol_minor != next_infra_mcp::LOCAL_RPC_PROTOCOL_MINOR
        || record.minimum_supported_minor != next_infra_mcp::LOCAL_RPC_MINIMUM_SUPPORTED_MINOR
        || record.stable_app_path != paths.stable_app
        || record.stable_bridge_path != paths.stable_bridge
        || !is_single_component(&record.release_id)
        || record.bundle_id.trim().is_empty()
        || record.app_designated_requirement.trim().is_empty()
        || record.bridge_designated_requirement.trim().is_empty()
        || record.installed_at.trim().is_empty()
        || record.updated_at.trim().is_empty()
    {
        return Err(AvailabilityError::Invalid("integration contract"));
    }

    let expected = next_infra_mcp::local_rpc_capability_names();
    validate_capabilities(&record.host_supported_capabilities, &expected)?;
    validate_capabilities(&record.bridge_required_capabilities, &expected)?;
    validate_capabilities(&record.host_required_capabilities, &[])?;
    validate_capabilities(&record.bridge_supported_capabilities, &[])?;
    if record.allow_mcp_auto_launch && record.team_id.as_deref().is_none_or(str::is_empty) {
        return Err(AvailabilityError::Invalid("auto-launch signing identity"));
    }
    Ok(())
}

fn validate_capabilities(actual: &[String], expected: &[&str]) -> Result<(), AvailabilityError> {
    if actual
        .iter()
        .map(String::as_str)
        .ne(expected.iter().copied())
    {
        return Err(AvailabilityError::Invalid("capability set"));
    }
    Ok(())
}

fn validate_current_release(
    paths: &IntegrationPaths,
    record: &IntegrationRecord,
    current_executable: &Path,
) -> Result<(), AvailabilityError> {
    let link_metadata = fs::symlink_metadata(&paths.current_link)
        .map_err(|error| map_missing(error, "current release link"))?;
    if !link_metadata.file_type().is_symlink() || link_metadata.uid() != current_euid() {
        return Err(AvailabilityError::Invalid("current release link"));
    }
    let target = fs::read_link(&paths.current_link).map_err(|_| AvailabilityError::Io)?;
    let expected_target = Path::new("releases").join(&record.release_id);
    if target != expected_target || target.is_absolute() || has_non_normal_component(&target) {
        return Err(AvailabilityError::Invalid("current release target"));
    }

    validate_directory(&paths.releases_dir.join(&record.release_id))?;

    let bridge_metadata = fs::symlink_metadata(&paths.stable_bridge)
        .map_err(|error| map_missing(error, "stable Bridge"))?;
    validate_executable(&paths.stable_bridge, &bridge_metadata)?;
    let resolved_bridge =
        fs::canonicalize(&paths.stable_bridge).map_err(|_| AvailabilityError::Io)?;
    let resolved_current =
        fs::canonicalize(current_executable).map_err(|_| AvailabilityError::Io)?;
    if resolved_bridge != resolved_current {
        return Err(AvailabilityError::Invalid("running Bridge release"));
    }
    Ok(())
}

fn validate_app_path(paths: &IntegrationPaths) -> Result<(), AvailabilityError> {
    let parent = paths
        .stable_app
        .parent()
        .ok_or(AvailabilityError::Invalid("Applications directory"))?;
    let parent_metadata = fs::symlink_metadata(parent)
        .map_err(|error| map_missing(error, "Applications directory"))?;
    if parent_metadata.file_type().is_symlink()
        || !parent_metadata.is_dir()
        || parent_metadata.uid() != current_euid()
    {
        return Err(AvailabilityError::Invalid("Applications directory"));
    }
    let metadata = fs::symlink_metadata(&paths.stable_app)
        .map_err(|error| map_missing(error, "stable App"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() || metadata.uid() != current_euid() {
        return Err(AvailabilityError::Invalid("stable App"));
    }
    Ok(())
}

fn validate_directory(path: &Path) -> Result<(), AvailabilityError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| map_missing(error, "integration directory"))?;
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || metadata.uid() != current_euid()
        || metadata.permissions().mode() & 0o7777 != DIRECTORY_MODE
    {
        return Err(AvailabilityError::Invalid("integration directory"));
    }
    Ok(())
}

fn validate_regular_file(
    _path: &Path,
    metadata: &fs::Metadata,
    mode: u32,
) -> Result<(), AvailabilityError> {
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.uid() != current_euid()
        || metadata.permissions().mode() & 0o7777 != mode
    {
        return Err(AvailabilityError::Invalid("integration file"));
    }
    Ok(())
}

fn validate_executable(_path: &Path, metadata: &fs::Metadata) -> Result<(), AvailabilityError> {
    let mode = metadata.permissions().mode() & 0o7777;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.uid() != current_euid()
        || mode & 0o022 != 0
        || mode & 0o100 == 0
    {
        return Err(AvailabilityError::Invalid("Bridge executable"));
    }
    Ok(())
}

fn map_missing(error: std::io::Error, label: &'static str) -> AvailabilityError {
    if error.kind() == std::io::ErrorKind::NotFound {
        AvailabilityError::Missing(label)
    } else {
        AvailabilityError::Io
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
