use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use next_infra_mcp::{LocalRpcMcpClient, McpBridgeError};

use super::{
    AvailabilityActionError, HostConnector, HostLauncher, IntegrationPaths, IntegrationRecord,
    MonotonicClock, SignatureVerifier, VerifiedArtifacts,
};

pub struct LocalRpcConnector {
    run_dir: PathBuf,
    bridge_version: String,
    release_id: String,
}

impl LocalRpcConnector {
    pub fn new(
        run_dir: impl Into<PathBuf>,
        bridge_version: impl Into<String>,
        release_id: impl Into<String>,
    ) -> Self {
        Self {
            run_dir: run_dir.into(),
            bridge_version: bridge_version.into(),
            release_id: release_id.into(),
        }
    }
}

impl HostConnector for LocalRpcConnector {
    type Client = LocalRpcMcpClient;

    fn connect(&self, timeout: Duration) -> Result<Self::Client, McpBridgeError> {
        LocalRpcMcpClient::connect_run_dir_with_timeout(
            self.run_dir.clone(),
            self.bridge_version.clone(),
            self.release_id.clone(),
            timeout,
        )
    }
}

pub struct SystemMonotonicClock {
    started_at: Instant,
}

impl Default for SystemMonotonicClock {
    fn default() -> Self {
        Self {
            started_at: Instant::now(),
        }
    }
}

impl MonotonicClock for SystemMonotonicClock {
    fn now(&self) -> Duration {
        self.started_at.elapsed()
    }

    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

#[derive(Debug, Default)]
pub struct MacOpenLauncher {
    attempted: AtomicBool,
}

impl HostLauncher for MacOpenLauncher {
    fn launch(&self, artifacts: &VerifiedArtifacts) -> Result<(), AvailabilityActionError> {
        self.claim_launch()?;
        artifacts.revalidate()?;
        let (bundle_id, team_id, requirement) = artifacts.app_signature_authorization()?;
        verify_artifact(
            artifacts.stable_app_path(),
            requirement,
            Some(bundle_id),
            team_id,
        )?;
        artifacts.revalidate()?;
        let (program, arguments) = open_command(artifacts.stable_app_path());
        let status = Command::new(program)
            .args(arguments)
            .status()
            .map_err(|_| AvailabilityActionError)?;
        status
            .success()
            .then_some(())
            .ok_or(AvailabilityActionError)
    }
}

impl MacOpenLauncher {
    fn claim_launch(&self) -> Result<(), AvailabilityActionError> {
        self.attempted
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| AvailabilityActionError)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MacCodeSignatureVerifier;

impl SignatureVerifier for MacCodeSignatureVerifier {
    fn verify(
        &self,
        paths: &IntegrationPaths,
        record: &IntegrationRecord,
        current_executable: &Path,
    ) -> Result<VerifiedArtifacts, AvailabilityActionError> {
        let team = record.team_id.as_deref().ok_or(AvailabilityActionError)?;
        let before = VerifiedArtifacts::capture(paths, current_executable)?;
        verify_artifact(
            &paths.stable_app,
            &record.app_designated_requirement,
            Some(&record.bundle_id),
            team,
        )?;
        verify_artifact(
            current_executable,
            &record.bridge_designated_requirement,
            None,
            team,
        )?;
        let after = VerifiedArtifacts::capture(paths, current_executable)?;
        (before == after)
            .then(|| {
                after.authorize_app_signature(
                    &record.bundle_id,
                    team,
                    &record.app_designated_requirement,
                )
            })
            .ok_or(AvailabilityActionError)
    }
}

pub fn open_command(stable_app: &Path) -> (&'static Path, Vec<OsString>) {
    (
        Path::new("/usr/bin/open"),
        vec![
            OsString::from("-g"),
            stable_app.as_os_str().to_owned(),
            OsString::from("--args"),
            OsString::from("--background"),
            OsString::from("--launch-source=mcp"),
        ],
    )
}

fn verify_artifact(
    artifact: &Path,
    requirement: &str,
    expected_identifier: Option<&str>,
    expected_team: &str,
) -> Result<(), AvailabilityActionError> {
    if requirement.trim().is_empty() || expected_team.trim().is_empty() {
        return Err(AvailabilityActionError);
    }
    let verify = Command::new("/usr/bin/codesign")
        .arg("--verify")
        .arg("--strict")
        .arg("--verbose=4")
        .arg(format!("-R={requirement}"))
        .arg(artifact)
        .output()
        .map_err(|_| AvailabilityActionError)?;
    if !verify.status.success() {
        return Err(AvailabilityActionError);
    }

    let details = codesign_output(["-d", "--verbose=4"], artifact)?;
    if unique_field(&details, "TeamIdentifier")? != expected_team {
        return Err(AvailabilityActionError);
    }
    if let Some(identifier) = expected_identifier
        && unique_field(&details, "Identifier")? != identifier
    {
        return Err(AvailabilityActionError);
    }

    let requirements = codesign_output(["-d", "-r-"], artifact)?;
    let designated = unique_prefixed_line(&requirements, "designated => ")?;
    (designated == requirement)
        .then_some(())
        .ok_or(AvailabilityActionError)
}

fn codesign_output<const N: usize>(
    arguments: [&str; N],
    artifact: &Path,
) -> Result<String, AvailabilityActionError> {
    let output = Command::new("/usr/bin/codesign")
        .args(arguments)
        .arg(artifact)
        .output()
        .map_err(|_| AvailabilityActionError)?;
    output_text(output)
}

fn output_text(output: Output) -> Result<String, AvailabilityActionError> {
    if !output.status.success() {
        return Err(AvailabilityActionError);
    }
    let mut bytes = output.stdout;
    bytes.push(b'\n');
    bytes.extend_from_slice(&output.stderr);
    String::from_utf8(bytes).map_err(|_| AvailabilityActionError)
}

fn unique_field<'a>(text: &'a str, name: &str) -> Result<&'a str, AvailabilityActionError> {
    unique_prefixed_line(text, &format!("{name}="))
}

fn unique_prefixed_line<'a>(
    text: &'a str,
    prefix: &str,
) -> Result<&'a str, AvailabilityActionError> {
    let mut matches = text
        .lines()
        .filter_map(|line| line.trim().strip_prefix(prefix));
    let value = matches.next().ok_or(AvailabilityActionError)?;
    if value.is_empty() || matches.next().is_some() {
        return Err(AvailabilityActionError);
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_launcher_latch_allows_only_one_attempt() {
        let launcher = MacOpenLauncher::default();
        assert_eq!(launcher.claim_launch(), Ok(()));
        assert_eq!(launcher.claim_launch(), Err(AvailabilityActionError));
    }

    #[test]
    fn codesign_output_fields_must_be_unique_and_nonempty() {
        assert_eq!(
            unique_field("Identifier=dev.guoxudong.next-infra\n", "Identifier"),
            Ok("dev.guoxudong.next-infra")
        );
        assert_eq!(
            unique_field("Identifier=one\nIdentifier=two\n", "Identifier"),
            Err(AvailabilityActionError)
        );
        assert_eq!(
            unique_field("Identifier=\n", "Identifier"),
            Err(AvailabilityActionError)
        );
        assert_eq!(
            unique_prefixed_line(
                "designated => identifier one\ndesignated => identifier two\n",
                "designated => ",
            ),
            Err(AvailabilityActionError)
        );
    }
}
