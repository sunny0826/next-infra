use crate::{
    MAX_BATCH_OUTPUT_BYTES, MAX_BATCH_WALL_TIME_SECS, MAX_PROBES_PER_BATCH, ProbeId,
    SshConnectionConfigV1, SshError, probe_spec, timeout_for, validate_registry,
};
use async_trait::async_trait;
use next_infra_connector_api::ConnectorFailure;
use next_infra_core::ErrorCode;
use std::{
    collections::{BTreeSet, HashMap},
    fmt, io,
    process::{ExitStatus, Stdio},
    sync::{
        Arc, LazyLock, Mutex as StdMutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::{Child, Command},
    sync::Notify,
};

const OPENSSH_EXECUTABLE: &str = "/usr/bin/ssh";
type ConnectionLock = tokio::sync::Mutex<()>;
static SSH_CONNECTION_LOCKS: LazyLock<StdMutex<HashMap<String, Weak<ConnectionLock>>>> =
    LazyLock::new(|| StdMutex::new(HashMap::new()));
const FIXED_OPTIONS: &[&str] = &[
    "-T",
    "-o",
    "BatchMode=yes",
    "-o",
    "StrictHostKeyChecking=yes",
    "-o",
    "ConnectionAttempts=1",
];
const FIXED_OPTIONS_AFTER_TIMEOUT: &[&str] = &[
    "-o",
    "NumberOfPasswordPrompts=0",
    "-o",
    "RequestTTY=no",
    "-o",
    "LogLevel=ERROR",
    "-o",
    "AddKeysToAgent=no",
    "-o",
    "ClearAllForwardings=yes",
    "-o",
    "ControlMaster=no",
    "-o",
    "ControlPath=none",
    "-o",
    "ControlPersist=no",
    "-o",
    "ForwardAgent=no",
    "-o",
    "ForwardX11=no",
    "-o",
    "PermitLocalCommand=no",
    "-o",
    "UpdateHostKeys=no",
];

#[derive(Clone, Default)]
pub struct SshCancellation {
    inner: Arc<CancellationState>,
}

#[derive(Default)]
struct CancellationState {
    cancelled: AtomicBool,
    notify: Notify,
}

impl SshCancellation {
    pub fn cancel(&self) {
        self.inner.cancelled.store(true, Ordering::Release);
        self.inner.notify.notify_waiters();
    }

    fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::Acquire)
    }

    async fn cancelled(&self) {
        let notified = self.inner.notify.notified();
        if self.inner.cancelled.load(Ordering::Acquire) {
            return;
        }
        notified.await;
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ProbeOutput {
    pub probe_id: ProbeId,
    stdout: Vec<u8>,
    pub elapsed_ms: u64,
}

impl ProbeOutput {
    pub fn from_collected_stdout(
        probe_id: ProbeId,
        stdout: Vec<u8>,
        elapsed_ms: u64,
    ) -> Result<Self, ConnectorFailure> {
        if stdout.len() > crate::probe_metadata(probe_id).stdout_limit_bytes {
            return Err(SshError::output_limit().into());
        }
        Ok(Self {
            probe_id,
            stdout,
            elapsed_ms,
        })
    }

    pub fn stdout(&self) -> &[u8] {
        &self.stdout
    }

    #[cfg(test)]
    pub(crate) fn synthetic(probe_id: ProbeId, stdout: impl Into<Vec<u8>>) -> Self {
        Self::from_collected_stdout(probe_id, stdout.into(), 1).unwrap()
    }
}

impl fmt::Debug for ProbeOutput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProbeOutput")
            .field("probe_id", &self.probe_id)
            .field("stdout_bytes", &self.stdout.len())
            .field("elapsed_ms", &self.elapsed_ms)
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProbeOutcome {
    Success(ProbeOutput),
    Failure {
        probe_id: ProbeId,
        failure: ConnectorFailure,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SshBatchOutput {
    pub outcomes: Vec<ProbeOutcome>,
    pub elapsed_ms: u64,
    pub output_bytes: usize,
}

pub struct OpenSshClient {
    executor: Arc<dyn SshExecutor>,
}

impl OpenSshClient {
    pub fn new() -> Self {
        Self {
            executor: Arc::new(TokioSshExecutor),
        }
    }

    #[cfg(test)]
    fn with_executor(executor: Arc<dyn SshExecutor>) -> Self {
        Self { executor }
    }

    pub async fn execute_batch(
        &self,
        config: &SshConnectionConfigV1,
        probes: &[ProbeId],
        cancellation: &SshCancellation,
    ) -> Result<SshBatchOutput, ConnectorFailure> {
        config.validate().map_err(ConnectorFailure::from)?;
        validate_registry().map_err(ConnectorFailure::from)?;
        validate_probe_batch(probes).map_err(ConnectorFailure::from)?;
        if cancellation.is_cancelled() {
            return Err(SshError::cancelled().into());
        }
        let connection_lock = connection_lock(&config.host_identity.as_str());
        let _batch_guard = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(SshError::cancelled().into()),
            guard = connection_lock.lock() => guard,
        };

        let batch_started = Instant::now();
        let batch_limit = Duration::from_secs(MAX_BATCH_WALL_TIME_SECS);
        let mut outcomes = Vec::with_capacity(probes.len());
        let mut output_bytes = 0usize;
        let mut reserved_output_bytes = 0usize;

        for probe_id in probes {
            if cancellation.is_cancelled() {
                return Err(SshError::cancelled().into());
            }
            let spec = probe_spec(*probe_id).map_err(ConnectorFailure::from)?;
            let elapsed = batch_started.elapsed();
            let Some(remaining_time) = batch_limit.checked_sub(elapsed) else {
                outcomes.push(failure_outcome(*probe_id, SshError::timeout()));
                break;
            };
            if remaining_time.is_zero() {
                outcomes.push(failure_outcome(*probe_id, SshError::timeout()));
                break;
            }

            let required_output = spec
                .metadata
                .stdout_limit_bytes
                .saturating_add(spec.metadata.stderr_limit_bytes);
            if reserved_output_bytes.saturating_add(required_output) > MAX_BATCH_OUTPUT_BYTES {
                outcomes.push(failure_outcome(*probe_id, SshError::output_limit()));
                break;
            }
            reserved_output_bytes = reserved_output_bytes.saturating_add(required_output);

            let request = ProcessRequest::for_probe(
                config,
                spec.command,
                timeout_for(*probe_id).min(remaining_time),
                spec.metadata.stdout_limit_bytes,
                spec.metadata.stderr_limit_bytes,
            );
            match self.executor.execute(request, cancellation).await {
                Ok(raw) => {
                    output_bytes = output_bytes
                        .saturating_add(raw.stdout.len())
                        .saturating_add(raw.stderr.len());
                    if raw.status.success() {
                        outcomes.push(ProbeOutcome::Success(ProbeOutput {
                            probe_id: *probe_id,
                            stdout: raw.stdout,
                            elapsed_ms: raw.elapsed_ms,
                        }));
                    } else {
                        let error = classify_openssh_failure(&raw.stderr);
                        if error.code == ErrorCode::HostKeyMismatch {
                            return Err(error.into());
                        }
                        outcomes.push(failure_outcome(*probe_id, error));
                    }
                }
                Err(error) => {
                    if matches!(
                        error.code,
                        ErrorCode::HostKeyMismatch | ErrorCode::Cancelled
                    ) {
                        return Err(error.into());
                    }
                    outcomes.push(failure_outcome(*probe_id, error));
                }
            }
        }

        Ok(SshBatchOutput {
            outcomes,
            elapsed_ms: saturating_millis(batch_started.elapsed()),
            output_bytes,
        })
    }
}

fn connection_lock(host_identity: &str) -> Arc<ConnectionLock> {
    let mut locks = SSH_CONNECTION_LOCKS
        .lock()
        .expect("SSH connection lock registry is not poisoned");
    locks.retain(|_, lock| lock.strong_count() > 0);
    if let Some(lock) = locks.get(host_identity).and_then(Weak::upgrade) {
        return lock;
    }
    let lock = Arc::new(ConnectionLock::new(()));
    locks.insert(host_identity.to_owned(), Arc::downgrade(&lock));
    lock
}

impl Default for OpenSshClient {
    fn default() -> Self {
        Self::new()
    }
}

fn validate_probe_batch(probes: &[ProbeId]) -> Result<(), SshError> {
    if probes.is_empty()
        || probes.len() > MAX_PROBES_PER_BATCH
        || probes.iter().collect::<BTreeSet<_>>().len() != probes.len()
    {
        return Err(SshError::invalid_config());
    }
    Ok(())
}

fn failure_outcome(probe_id: ProbeId, error: SshError) -> ProbeOutcome {
    ProbeOutcome::Failure {
        probe_id,
        failure: error.into(),
    }
}

struct ProcessRequest {
    executable: &'static str,
    args: Vec<String>,
    timeout: Duration,
    stdout_limit: usize,
    stderr_limit: usize,
}

impl ProcessRequest {
    fn for_probe(
        config: &SshConnectionConfigV1,
        remote_command: &'static str,
        timeout: Duration,
        stdout_limit: usize,
        stderr_limit: usize,
    ) -> Self {
        let mut args = FIXED_OPTIONS
            .iter()
            .map(|value| (*value).to_owned())
            .collect::<Vec<_>>();
        args.push("-o".into());
        args.push(format!("ConnectTimeout={}", config.connect_timeout_secs));
        args.extend(
            FIXED_OPTIONS_AFTER_TIMEOUT
                .iter()
                .map(|value| (*value).to_owned()),
        );
        args.push(config.host_alias.expose().to_owned());
        args.push(remote_command.to_owned());
        Self {
            executable: OPENSSH_EXECUTABLE,
            args,
            timeout,
            stdout_limit,
            stderr_limit,
        }
    }

    #[cfg(test)]
    fn test_shell(
        script: &'static str,
        timeout: Duration,
        stdout_limit: usize,
        stderr_limit: usize,
    ) -> Self {
        Self {
            executable: "/bin/sh",
            args: vec!["-c".into(), script.into()],
            timeout,
            stdout_limit,
            stderr_limit,
        }
    }
}

impl fmt::Debug for ProcessRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProcessRequest")
            .field("executable", &"system-openssh")
            .field("arg_count", &self.args.len())
            .field("timeout", &self.timeout)
            .field("stdout_limit", &self.stdout_limit)
            .field("stderr_limit", &self.stderr_limit)
            .finish()
    }
}

struct RawProcessOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    elapsed_ms: u64,
}

impl fmt::Debug for RawProcessOutput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RawProcessOutput")
            .field("status", &self.status)
            .field("stdout_bytes", &self.stdout.len())
            .field("stderr_bytes", &self.stderr.len())
            .field("elapsed_ms", &self.elapsed_ms)
            .finish()
    }
}

#[async_trait]
trait SshExecutor: Send + Sync {
    async fn execute(
        &self,
        request: ProcessRequest,
        cancellation: &SshCancellation,
    ) -> Result<RawProcessOutput, SshError>;
}

struct TokioSshExecutor;

#[async_trait]
impl SshExecutor for TokioSshExecutor {
    async fn execute(
        &self,
        request: ProcessRequest,
        cancellation: &SshCancellation,
    ) -> Result<RawProcessOutput, SshError> {
        if cancellation.is_cancelled() {
            return Err(SshError::cancelled());
        }
        let started = Instant::now();
        let mut command = Command::new(request.executable);
        command
            .args(&request.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .env("LC_ALL", "C")
            .env("LANG", "C")
            .env("SSH_ASKPASS_REQUIRE", "never")
            .env_remove("SSH_ASKPASS");
        let mut child = command.spawn().map_err(|_| SshError::internal())?;
        let Some(stdout) = child.stdout.take() else {
            kill_and_reap(&mut child).await;
            return Err(SshError::internal());
        };
        let Some(stderr) = child.stderr.take() else {
            kill_and_reap(&mut child).await;
            return Err(SshError::internal());
        };

        enum Termination {
            Completed(io::Result<(ExitStatus, Vec<u8>, Vec<u8>)>),
            TimedOut,
            Cancelled,
        }

        let termination = {
            let execution = async {
                tokio::try_join!(
                    child.wait(),
                    read_limited(stdout, request.stdout_limit),
                    read_limited(stderr, request.stderr_limit),
                )
            };
            tokio::pin!(execution);
            tokio::select! {
                biased;
                _ = cancellation.cancelled() => Termination::Cancelled,
                result = &mut execution => Termination::Completed(result),
                _ = tokio::time::sleep(request.timeout) => Termination::TimedOut,
            }
        };

        match termination {
            Termination::Completed(Ok((status, stdout, stderr))) => Ok(RawProcessOutput {
                status,
                stdout,
                stderr,
                elapsed_ms: saturating_millis(started.elapsed()),
            }),
            Termination::Completed(Err(error)) => {
                kill_and_reap(&mut child).await;
                if error.kind() == io::ErrorKind::InvalidData {
                    Err(SshError::output_limit())
                } else {
                    Err(SshError::remote_failure())
                }
            }
            Termination::TimedOut => {
                kill_and_reap(&mut child).await;
                Err(SshError::timeout())
            }
            Termination::Cancelled => {
                kill_and_reap(&mut child).await;
                Err(SshError::cancelled())
            }
        }
    }
}

async fn read_limited<R>(mut reader: R, limit: usize) -> io::Result<Vec<u8>>
where
    R: AsyncRead + Unpin,
{
    let mut output = Vec::with_capacity(limit.min(8192));
    let mut buffer = [0u8; 8192];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            return Ok(output);
        }
        if output.len().saturating_add(read) > limit {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "output limit"));
        }
        output.extend_from_slice(&buffer[..read]);
    }
}

async fn kill_and_reap(child: &mut Child) {
    let _ = child.start_kill();
    let _ = child.wait().await;
}

fn classify_openssh_failure(stderr: &[u8]) -> SshError {
    let text = String::from_utf8_lossy(stderr);
    if [
        "REMOTE HOST IDENTIFICATION HAS CHANGED",
        "Host key verification failed",
        "host key is known",
    ]
    .iter()
    .any(|signature| text.contains(signature))
    {
        return SshError::host_key_mismatch();
    }
    if [
        "Permission denied",
        "Authentication failed",
        "Too many authentication failures",
    ]
    .iter()
    .any(|signature| text.contains(signature))
    {
        return SshError::authentication();
    }
    if [
        "Could not resolve hostname",
        "Connection timed out",
        "Operation timed out",
        "Connection refused",
        "No route to host",
        "Network is unreachable",
    ]
    .iter()
    .any(|signature| text.contains(signature))
    {
        return SshError::network();
    }
    SshError::remote_failure()
}

fn saturating_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ProbeProfile;
    use std::{
        collections::VecDeque,
        os::unix::process::ExitStatusExt,
        sync::{
            Mutex,
            atomic::{AtomicUsize, Ordering as AtomicOrdering},
        },
    };

    struct FakeExecutor {
        outputs: Mutex<VecDeque<Result<RawProcessOutput, SshError>>>,
        requests: Mutex<Vec<ProcessRequest>>,
    }

    #[async_trait]
    impl SshExecutor for FakeExecutor {
        async fn execute(
            &self,
            request: ProcessRequest,
            _cancellation: &SshCancellation,
        ) -> Result<RawProcessOutput, SshError> {
            self.requests.lock().unwrap().push(request);
            self.outputs.lock().unwrap().pop_front().unwrap()
        }
    }

    fn config() -> SshConnectionConfigV1 {
        config_with_identity("9f7fd5e6-3bc8-4daa-ae6b-9dfdffb54743")
    }

    fn config_with_identity(identity: &str) -> SshConnectionConfigV1 {
        SshConnectionConfigV1 {
            host_identity: crate::HostIdentity::parse(identity).unwrap(),
            host_alias: crate::HostAlias::parse("fixture-host").unwrap(),
            connect_timeout_secs: 10,
            probe_profile: ProbeProfile::BaselineV1,
            allowed_service_ids: Vec::new(),
        }
    }

    fn raw(code: i32, stdout: &[u8], stderr: &[u8]) -> RawProcessOutput {
        RawProcessOutput {
            status: ExitStatus::from_raw(code << 8),
            stdout: stdout.to_vec(),
            stderr: stderr.to_vec(),
            elapsed_ms: 1,
        }
    }

    #[tokio::test]
    async fn invocation_is_fixed_and_dynamic_values_are_separate_argv() {
        let executor = Arc::new(FakeExecutor {
            outputs: Mutex::new(VecDeque::from([Ok(raw(0, b"Darwin\narm64\n", b""))])),
            requests: Mutex::new(Vec::new()),
        });
        let client = OpenSshClient::with_executor(executor.clone());
        client
            .execute_batch(
                &config(),
                &[ProbeId::HostIdentityV1],
                &SshCancellation::default(),
            )
            .await
            .unwrap();
        let requests = executor.requests.lock().unwrap();
        let request = &requests[0];
        assert_eq!(request.executable, OPENSSH_EXECUTABLE);
        let command = crate::probe_spec(ProbeId::HostIdentityV1).unwrap().command;
        assert_eq!(
            request.args,
            [
                "-T",
                "-o",
                "BatchMode=yes",
                "-o",
                "StrictHostKeyChecking=yes",
                "-o",
                "ConnectionAttempts=1",
                "-o",
                "ConnectTimeout=10",
                "-o",
                "NumberOfPasswordPrompts=0",
                "-o",
                "RequestTTY=no",
                "-o",
                "LogLevel=ERROR",
                "-o",
                "AddKeysToAgent=no",
                "-o",
                "ClearAllForwardings=yes",
                "-o",
                "ControlMaster=no",
                "-o",
                "ControlPath=none",
                "-o",
                "ControlPersist=no",
                "-o",
                "ForwardAgent=no",
                "-o",
                "ForwardX11=no",
                "-o",
                "PermitLocalCommand=no",
                "-o",
                "UpdateHostKeys=no",
                "fixture-host",
                command,
            ]
            .map(str::to_owned)
        );
        assert!(!format!("{request:?}").contains("fixture-host"));
    }

    #[tokio::test]
    async fn nonfatal_probe_failure_is_preserved_but_host_key_is_fatal() {
        let partial = Arc::new(FakeExecutor {
            outputs: Mutex::new(VecDeque::from([
                Ok(raw(0, b"ok", b"")),
                Ok(raw(255, b"", b"Connection refused output-sentinel")),
            ])),
            requests: Mutex::new(Vec::new()),
        });
        let result = OpenSshClient::with_executor(partial)
            .execute_batch(
                &config(),
                &[ProbeId::HostIdentityV1, ProbeId::HostUptimeV1],
                &SshCancellation::default(),
            )
            .await
            .unwrap();
        assert!(matches!(result.outcomes[0], ProbeOutcome::Success(_)));
        let ProbeOutcome::Failure { failure, .. } = &result.outcomes[1] else {
            panic!()
        };
        assert_eq!(failure.code, ErrorCode::NetworkUnreachable);
        assert!(!format!("{failure:?}").contains("output-sentinel"));

        let fatal = Arc::new(FakeExecutor {
            outputs: Mutex::new(VecDeque::from([Ok(raw(
                255,
                b"",
                b"REMOTE HOST IDENTIFICATION HAS CHANGED output-sentinel",
            ))])),
            requests: Mutex::new(Vec::new()),
        });
        let failure = OpenSshClient::with_executor(fatal)
            .execute_batch(
                &config(),
                &[ProbeId::HostIdentityV1],
                &SshCancellation::default(),
            )
            .await
            .unwrap_err();
        assert_eq!(failure.code, ErrorCode::HostKeyMismatch);
        assert!(!format!("{failure:?}").contains("output-sentinel"));
    }

    #[tokio::test]
    async fn batch_rejects_empty_duplicate_and_excessive_probes_before_spawn() {
        let executor = Arc::new(FakeExecutor {
            outputs: Mutex::new(VecDeque::new()),
            requests: Mutex::new(Vec::new()),
        });
        let client = OpenSshClient::with_executor(executor.clone());
        for probes in [
            Vec::new(),
            vec![ProbeId::HostIdentityV1, ProbeId::HostIdentityV1],
            vec![ProbeId::HostIdentityV1; MAX_PROBES_PER_BATCH + 1],
        ] {
            assert!(
                client
                    .execute_batch(&config(), &probes, &SshCancellation::default())
                    .await
                    .is_err()
            );
        }
        assert!(executor.requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn pre_cancelled_batch_never_spawns() {
        let executor = Arc::new(FakeExecutor {
            outputs: Mutex::new(VecDeque::new()),
            requests: Mutex::new(Vec::new()),
        });
        let client = OpenSshClient::with_executor(executor.clone());
        let cancellation = SshCancellation::default();
        cancellation.cancel();
        let failure = client
            .execute_batch(&config(), &[ProbeId::HostIdentityV1], &cancellation)
            .await
            .unwrap_err();
        assert_eq!(failure.code, ErrorCode::Cancelled);
        assert!(executor.requests.lock().unwrap().is_empty());
    }

    struct CancelAfterSuccessExecutor;

    #[async_trait]
    impl SshExecutor for CancelAfterSuccessExecutor {
        async fn execute(
            &self,
            _request: ProcessRequest,
            cancellation: &SshCancellation,
        ) -> Result<RawProcessOutput, SshError> {
            cancellation.cancel();
            Ok(raw(0, b"ok", b""))
        }
    }

    #[tokio::test]
    async fn cancellation_after_a_success_is_top_level_and_stops_the_batch() {
        let client = OpenSshClient::with_executor(Arc::new(CancelAfterSuccessExecutor));
        let cancellation = SshCancellation::default();
        let failure = client
            .execute_batch(
                &config(),
                &[ProbeId::HostIdentityV1, ProbeId::HostUptimeV1],
                &cancellation,
            )
            .await
            .unwrap_err();
        assert_eq!(failure.code, ErrorCode::Cancelled);
    }

    #[tokio::test]
    async fn real_executor_enforces_output_timeout_and_explicit_cancellation() {
        let executor = TokioSshExecutor;
        let overflow = executor
            .execute(
                ProcessRequest::test_shell("printf 12345", Duration::from_secs(1), 4, 32),
                &SshCancellation::default(),
            )
            .await
            .unwrap_err();
        assert_eq!(overflow.code, ErrorCode::InvalidResponse);

        let stderr_overflow = executor
            .execute(
                ProcessRequest::test_shell("printf 12345 >&2", Duration::from_secs(1), 32, 4),
                &SshCancellation::default(),
            )
            .await
            .unwrap_err();
        assert_eq!(stderr_overflow.code, ErrorCode::InvalidResponse);

        let timeout = executor
            .execute(
                ProcessRequest::test_shell("sleep 5", Duration::from_millis(10), 32, 32),
                &SshCancellation::default(),
            )
            .await
            .unwrap_err();
        assert_eq!(timeout.code, ErrorCode::ProviderUnavailable);

        let cancellation = SshCancellation::default();
        let trigger = cancellation.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            trigger.cancel();
        });
        let cancelled = executor
            .execute(
                ProcessRequest::test_shell("sleep 5", Duration::from_secs(2), 32, 32),
                &cancellation,
            )
            .await
            .unwrap_err();
        assert_eq!(cancelled.code, ErrorCode::Cancelled);
    }

    #[test]
    fn openssh_error_signatures_map_without_echoing_stderr() {
        let cases = [
            (
                b"Permission denied output-sentinel".as_slice(),
                ErrorCode::AuthenticationFailed,
                false,
            ),
            (
                b"Could not resolve hostname output-sentinel".as_slice(),
                ErrorCode::NetworkUnreachable,
                true,
            ),
            (
                b"unknown ssh failure output-sentinel".as_slice(),
                ErrorCode::ProviderUnavailable,
                false,
            ),
        ];
        for (stderr, code, retryable) in cases {
            let error = classify_openssh_failure(stderr);
            assert_eq!(error.code, code);
            assert_eq!(error.retryable, retryable);
            assert!(!format!("{error:?}").contains("output-sentinel"));
        }
    }

    struct ConcurrencyExecutor {
        active: AtomicUsize,
        maximum: AtomicUsize,
    }

    #[async_trait]
    impl SshExecutor for ConcurrencyExecutor {
        async fn execute(
            &self,
            _request: ProcessRequest,
            _cancellation: &SshCancellation,
        ) -> Result<RawProcessOutput, SshError> {
            let active = self.active.fetch_add(1, AtomicOrdering::SeqCst) + 1;
            self.maximum.fetch_max(active, AtomicOrdering::SeqCst);
            tokio::time::sleep(Duration::from_millis(20)).await;
            self.active.fetch_sub(1, AtomicOrdering::SeqCst);
            Ok(raw(0, b"ok", b""))
        }
    }

    #[tokio::test]
    async fn concurrent_batches_still_run_only_one_child() {
        let executor = Arc::new(ConcurrencyExecutor {
            active: AtomicUsize::new(0),
            maximum: AtomicUsize::new(0),
        });
        let first_client = OpenSshClient::with_executor(executor.clone());
        let second_client = OpenSshClient::with_executor(executor.clone());
        let first_config = config_with_identity("33333333-3333-4333-8333-333333333333");
        let second_config = config_with_identity("33333333-3333-4333-8333-333333333333");
        let first_cancel = SshCancellation::default();
        let second_cancel = SshCancellation::default();
        let first =
            first_client.execute_batch(&first_config, &[ProbeId::HostIdentityV1], &first_cancel);
        let second =
            second_client.execute_batch(&second_config, &[ProbeId::HostUptimeV1], &second_cancel);
        let (first, second) = tokio::join!(first, second);
        first.unwrap();
        second.unwrap();
        assert_eq!(executor.maximum.load(AtomicOrdering::SeqCst), 1);
    }

    #[tokio::test]
    async fn different_connections_may_run_concurrently() {
        let executor = Arc::new(ConcurrencyExecutor {
            active: AtomicUsize::new(0),
            maximum: AtomicUsize::new(0),
        });
        let first_client = OpenSshClient::with_executor(executor.clone());
        let second_client = OpenSshClient::with_executor(executor.clone());
        let first_config = config_with_identity("11111111-1111-4111-8111-111111111111");
        let second_config = config_with_identity("22222222-2222-4222-8222-222222222222");
        let first_cancel = SshCancellation::default();
        let second_cancel = SshCancellation::default();
        let first =
            first_client.execute_batch(&first_config, &[ProbeId::HostIdentityV1], &first_cancel);
        let second =
            second_client.execute_batch(&second_config, &[ProbeId::HostUptimeV1], &second_cancel);
        let (first, second) = tokio::join!(first, second);
        first.unwrap();
        second.unwrap();
        assert_eq!(executor.maximum.load(AtomicOrdering::SeqCst), 2);
    }
}
