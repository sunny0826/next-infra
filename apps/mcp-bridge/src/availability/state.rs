use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

use next_infra_mcp::McpBridgeError;

use super::{
    IntegrationPaths, IntegrationRecord, UserQuitInspection, inspect_user_quit,
    validate_integration_record,
};

const MAX_AVAILABILITY_TIMEOUT: Duration = Duration::from_secs(10);
const INITIAL_CONNECT_TIMEOUT: Duration = Duration::from_millis(500);

pub trait HostConnector {
    type Client;
    fn connect(&self, timeout: Duration) -> Result<Self::Client, McpBridgeError>;
}

pub trait SignatureVerifier {
    fn verify(
        &self,
        paths: &IntegrationPaths,
        record: &IntegrationRecord,
        current_executable: &Path,
    ) -> Result<VerifiedArtifacts, AvailabilityActionError>;
}

pub trait HostLauncher {
    type Guard;
    fn coordinate(
        &self,
        paths: &IntegrationPaths,
    ) -> Result<Option<Self::Guard>, AvailabilityActionError>;
    fn launch(&self, artifacts: &VerifiedArtifacts) -> Result<(), AvailabilityActionError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AvailabilityActionError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedArtifacts {
    stable_app_path: PathBuf,
    current_executable: PathBuf,
    stable_app_identity: ArtifactIdentity,
    current_executable_identity: ArtifactIdentity,
    signature_authorization: Option<SignatureAuthorization>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ArtifactIdentity {
    device: u64,
    inode: u64,
    owner: u32,
    mode: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SignatureAuthorization {
    bundle_id: String,
    team_id: String,
    designated_requirement: String,
}

impl VerifiedArtifacts {
    #[doc(hidden)]
    pub fn capture(
        paths: &IntegrationPaths,
        current_executable: &Path,
    ) -> Result<Self, AvailabilityActionError> {
        let stable_app_identity = artifact_identity(&paths.stable_app, ArtifactKind::Directory)?;
        let current_executable_identity =
            artifact_identity(current_executable, ArtifactKind::RegularFile)?;
        Ok(Self {
            stable_app_path: paths.stable_app.clone(),
            current_executable: current_executable.to_path_buf(),
            stable_app_identity,
            current_executable_identity,
            signature_authorization: None,
        })
    }

    pub(crate) fn revalidate(&self) -> Result<(), AvailabilityActionError> {
        if artifact_identity(&self.stable_app_path, ArtifactKind::Directory)?
            != self.stable_app_identity
            || artifact_identity(&self.current_executable, ArtifactKind::RegularFile)?
                != self.current_executable_identity
        {
            return Err(AvailabilityActionError);
        }
        Ok(())
    }

    pub fn stable_app_path(&self) -> &Path {
        &self.stable_app_path
    }

    pub(crate) fn authorize_app_signature(
        mut self,
        bundle_id: &str,
        team_id: &str,
        designated_requirement: &str,
    ) -> Self {
        self.signature_authorization = Some(SignatureAuthorization {
            bundle_id: bundle_id.to_owned(),
            team_id: team_id.to_owned(),
            designated_requirement: designated_requirement.to_owned(),
        });
        self
    }

    pub(crate) fn app_signature_authorization(
        &self,
    ) -> Result<(&str, &str, &str), AvailabilityActionError> {
        let authorization = self
            .signature_authorization
            .as_ref()
            .ok_or(AvailabilityActionError)?;
        Ok((
            &authorization.bundle_id,
            &authorization.team_id,
            &authorization.designated_requirement,
        ))
    }
}

#[derive(Clone, Copy)]
enum ArtifactKind {
    Directory,
    RegularFile,
}

fn artifact_identity(
    path: &Path,
    expected: ArtifactKind,
) -> Result<ArtifactIdentity, AvailabilityActionError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| AvailabilityActionError)?;
    let mode = metadata.permissions().mode() & 0o7777;
    if metadata.file_type().is_symlink()
        || metadata.uid() != current_euid()
        || match expected {
            ArtifactKind::Directory => !metadata.is_dir(),
            ArtifactKind::RegularFile => {
                !metadata.is_file() || mode & 0o022 != 0 || mode & 0o100 == 0
            }
        }
    {
        return Err(AvailabilityActionError);
    }
    Ok(ArtifactIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
        owner: metadata.uid(),
        mode,
    })
}

fn current_euid() -> u32 {
    unsafe { libc::geteuid() as u32 }
}

pub trait MonotonicClock {
    fn now(&self) -> Duration;
    fn sleep(&self, duration: Duration);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AvailabilityPolicy {
    pub timeout: Duration,
    pub initial_delay: Duration,
    pub maximum_delay: Duration,
}

impl Default for AvailabilityPolicy {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(10),
            initial_delay: Duration::from_millis(50),
            maximum_delay: Duration::from_millis(500),
        }
    }
}

pub fn ensure_host<C, V, L, K>(
    paths: &IntegrationPaths,
    current_executable: &Path,
    connector: &C,
    verifier: &V,
    launcher: &L,
    clock: &K,
    policy: AvailabilityPolicy,
) -> Result<C::Client, McpBridgeError>
where
    C: HostConnector,
    V: SignatureVerifier,
    L: HostLauncher,
    K: MonotonicClock,
{
    let timeout = policy.timeout.min(MAX_AVAILABILITY_TIMEOUT);
    if timeout.is_zero() || policy.initial_delay.is_zero() || policy.maximum_delay.is_zero() {
        return Err(host_unavailable());
    }
    if let Ok(client) = connector.connect(timeout.min(INITIAL_CONNECT_TIMEOUT)) {
        return Ok(client);
    }
    if inspect_user_quit(paths) != UserQuitInspection::Clear {
        return Err(host_unavailable());
    }
    let record =
        validate_integration_record(paths, current_executable).map_err(|_| host_unavailable())?;
    if !record.allow_mcp_auto_launch {
        return Err(host_unavailable());
    }
    let launch_guard = launcher.coordinate(paths).map_err(|_| host_unavailable())?;
    if launch_guard.is_some() {
        let verified = verifier
            .verify(paths, &record, current_executable)
            .map_err(|_| host_unavailable())?;
        if inspect_user_quit(paths) != UserQuitInspection::Clear {
            return Err(host_unavailable());
        }
        verified.revalidate().map_err(|_| host_unavailable())?;
        launcher.launch(&verified).map_err(|_| host_unavailable())?;
    }
    let _launch_guard = launch_guard;

    let deadline = clock
        .now()
        .checked_add(timeout)
        .ok_or_else(host_unavailable)?;
    let mut delay = policy.initial_delay.min(policy.maximum_delay);
    loop {
        if inspect_user_quit(paths) != UserQuitInspection::Clear {
            return Err(host_unavailable());
        }
        if clock.now() >= deadline {
            return Err(host_unavailable());
        }
        let remaining = deadline.saturating_sub(clock.now());
        if let Ok(client) = connector.connect(remaining) {
            if inspect_user_quit(paths) != UserQuitInspection::Clear {
                return Err(host_unavailable());
            }
            if clock.now() >= deadline {
                return Err(host_unavailable());
            }
            return Ok(client);
        }
        let now = clock.now();
        if now >= deadline {
            return Err(host_unavailable());
        }
        clock.sleep(delay.min(deadline.saturating_sub(now)));
        delay = delay.saturating_mul(2).min(policy.maximum_delay);
    }
}

fn host_unavailable() -> McpBridgeError {
    McpBridgeError::new(
        "host_unavailable",
        "Next Infra is unavailable. Start it interactively or review MCP integration settings.",
        true,
    )
}
