use crate::{
    OpenSshClient, ProbeId, ProbeOutcome, SshBatchOutput, SshCancellation, SshConnectionConfigV1,
    probes::{
        common::{CommonModuleState, CommonProbeInput, HostPlatform, map_common, parse_identity},
        linux::map_systemd_services,
        macos::map_launchd_services,
    },
    ssh_descriptor,
};
use async_trait::async_trait;
use next_infra_connector_api::*;
use next_infra_core::*;
use std::collections::{BTreeMap, BTreeSet};
use std::time::{SystemTime, UNIX_EPOCH};

type ProbeOutputs = BTreeMap<ProbeId, Vec<u8>>;
type ModuleFailures = Vec<(&'static str, ConnectorFailure)>;

#[async_trait]
pub trait SshProbeClient: Send + Sync {
    async fn execute_batch(
        &self,
        config: &SshConnectionConfigV1,
        probes: &[ProbeId],
        cancellation: &SshCancellation,
    ) -> ConnectorResult<SshBatchOutput>;
}

#[async_trait]
impl SshProbeClient for OpenSshClient {
    async fn execute_batch(
        &self,
        config: &SshConnectionConfigV1,
        probes: &[ProbeId],
        cancellation: &SshCancellation,
    ) -> ConnectorResult<SshBatchOutput> {
        OpenSshClient::execute_batch(self, config, probes, cancellation).await
    }
}

pub trait SshClock: Send + Sync {
    fn now(&self) -> Timestamp;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemSshClock;

impl SshClock for SystemSshClock {
    fn now(&self) -> Timestamp {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        Timestamp::from_unix_millis(i64::try_from(millis).unwrap_or(i64::MAX))
            .expect("system time is non-negative")
    }
}

pub struct SshConnector<T, C = SystemSshClock> {
    descriptor: ConnectorDescriptor,
    client: T,
    clock: C,
}

impl<T> SshConnector<T, SystemSshClock> {
    pub fn new(client: T) -> Self {
        Self::with_clock(client, SystemSshClock)
    }
}

impl<T, C> SshConnector<T, C> {
    pub fn with_clock(client: T, clock: C) -> Self {
        Self {
            descriptor: ssh_descriptor(),
            client,
            clock,
        }
    }
}

#[async_trait]
impl<T, C> ReadConnector for SshConnector<T, C>
where
    T: SshProbeClient,
    C: SshClock,
{
    fn descriptor(&self) -> &ConnectorDescriptor {
        &self.descriptor
    }

    async fn validate(
        &self,
        request: ValidationRequest,
        _secret: Option<&SecretValue>,
    ) -> ConnectorResult<ValidationReport> {
        let config = match validate_connection(&request.connection) {
            Ok(config) => config,
            Err(issue) => return Ok(invalid_report(issue)),
        };
        let batch = self
            .client
            .execute_batch(
                &config,
                &[ProbeId::HostIdentityV1],
                &SshCancellation::default(),
            )
            .await;
        match batch.and_then(identity_stdout) {
            Ok(stdout) if parse_identity(&stdout).is_ok() => Ok(ValidationReport {
                status: ValidationStatus::Valid,
                warnings: Vec::new(),
                errors: Vec::new(),
            }),
            Ok(_) => Ok(invalid_report(invalid_response())),
            Err(failure) => Ok(invalid_report(failure)),
        }
    }

    async fn sync(
        &self,
        request: SyncRequest,
        _secret: Option<&SecretValue>,
    ) -> ConnectorResult<SyncOutcome> {
        let config = validate_connection(&request.connection)?;
        validate_sync_request(&request, &config)?;
        let cancellation = SshCancellation::default();
        let identity_batch = self
            .client
            .execute_batch(&config, &[ProbeId::HostIdentityV1], &cancellation)
            .await?;
        let mut summary = ProviderRequestSummary::default();
        add_summary(&mut summary, &identity_batch);
        let identity_stdout = identity_stdout(identity_batch)?;
        let identity =
            parse_identity(&identity_stdout).map_err(|failure| failure.connector_failure())?;

        let mut planned = vec![
            ProbeId::HostUptimeV1,
            ProbeId::HostFilesystemsV1,
            ProbeId::HostProcessSummaryV1,
        ];
        if !config.allowed_service_ids.is_empty() {
            planned.push(match identity.platform {
                HostPlatform::Darwin => ProbeId::MacosLaunchdServicesV1,
                HostPlatform::Linux => ProbeId::LinuxSystemdServicesV1,
            });
        }
        let child_batch = self
            .client
            .execute_batch(&config, &planned, &cancellation)
            .await?;
        add_summary(&mut summary, &child_batch);
        let (outputs, mut failures) = split_outcomes(child_batch, &planned)?;
        let observed_at = self.clock.now();
        let common = map_common(
            &config.host_identity,
            &config.host_alias,
            &request.scope,
            observed_at,
            CommonProbeInput {
                identity: Some(&identity_stdout),
                uptime: output(&outputs, ProbeId::HostUptimeV1),
                filesystems: output(&outputs, ProbeId::HostFilesystemsV1),
                process_summary: output(&outputs, ProbeId::HostProcessSummaryV1),
            },
        )
        .map_err(|failure| failure.connector_failure())?;
        for module in &common.modules {
            if module.state == CommonModuleState::Partial {
                failures.push((module.module, invalid_response()));
            }
        }

        let mut resources = common.resources;
        let mut relations = common.relations;
        if !config.allowed_service_ids.is_empty() {
            let service_result = match identity.platform {
                HostPlatform::Darwin => output(&outputs, ProbeId::MacosLaunchdServicesV1)
                    .ok_or_else(invalid_response)
                    .and_then(|stdout| {
                        map_launchd_services(
                            &config.host_identity,
                            &request.scope,
                            observed_at,
                            stdout,
                            &config.allowed_service_ids,
                        )
                        .map_err(|failure| failure.connector_failure())
                    }),
                HostPlatform::Linux => output(&outputs, ProbeId::LinuxSystemdServicesV1)
                    .ok_or_else(invalid_response)
                    .and_then(|stdout| {
                        map_systemd_services(
                            &config.host_identity,
                            &request.scope,
                            observed_at,
                            stdout,
                            &config.allowed_service_ids,
                        )
                        .map_err(|failure| failure.connector_failure())
                    }),
            };
            match service_result {
                Ok((mut service_resources, mut service_relations)) => {
                    resources.append(&mut service_resources);
                    relations.append(&mut service_relations);
                }
                Err(failure) => failures.push((service_module(identity.platform), failure)),
            }
        }

        sort_and_validate(&mut resources, &mut relations)?;
        let targeted = request.mode == SyncMode::Targeted;
        if targeted {
            failures.push(("ssh.targeted", targeted_partial_failure()));
        }
        let warnings = failures
            .iter()
            .map(|(module, failure)| ObservationWarning {
                code: failure.code,
                message: format!("SSH module {module} is partial"),
            })
            .collect::<Vec<_>>();
        let primary_failure = failures.first().map(|(_, failure)| failure.clone());
        let coverage = match &primary_failure {
            Some(failure) => SyncCoverage::Partial {
                scope: Some(request.scope.clone()),
                reason: coverage_reason(failure.code),
            },
            None => SyncCoverage::AuthoritativeFull {
                scope: request.scope.clone(),
            },
        };
        let batch = ObservationBatch {
            resources,
            relations,
            coverage,
            next_cursor: None,
            warnings,
            redaction_report: RedactionReport::default(),
            provider_request_summary: summary,
        };
        let outcome = match primary_failure {
            Some(failure) => SyncOutcome::Partial { batch, failure },
            None => SyncOutcome::Complete { batch },
        };
        outcome
            .validate_for(&request)
            .map_err(|_| invalid_response())?;
        Ok(outcome)
    }
}

fn validate_connection(connection: &ConnectionInput) -> ConnectorResult<SshConnectionConfigV1> {
    if connection.connector_type != ConnectorType::new("ssh").expect("static connector") {
        return Err(invalid_config(
            "SSH connection uses a different connector type",
        ));
    }
    if connection.config_schema_version != SchemaVersion::new(1).expect("static schema") {
        return Err(ConnectorFailure {
            code: ErrorCode::SchemaIncompatible,
            message: "SSH connection config schema is unsupported".into(),
            retryable: false,
            retry_after_ms: None,
        });
    }
    SshConnectionConfigV1::from_json(connection.config.clone()).map_err(ConnectorFailure::from)
}

fn validate_sync_request(
    request: &SyncRequest,
    config: &SshConnectionConfigV1,
) -> ConnectorResult<()> {
    if request.cursor.is_some() || request.mode == SyncMode::Incremental {
        return Err(invalid_config(
            "SSH incremental sync and cursors are unsupported",
        ));
    }
    if request.mode == SyncMode::Targeted
        && (request.targeted_resources.len() != 1
            || request.targeted_resources[0].kind
                != ResourceKind::new("ssh.host").expect("static kind")
            || request.targeted_resources[0].external_id != config.host_identity.external_id())
    {
        return Err(invalid_config(
            "SSH targeted sync requires the configured host locator",
        ));
    }
    if request.mode == SyncMode::Full && !request.targeted_resources.is_empty() {
        return Err(invalid_config(
            "SSH full sync does not accept targeted locators",
        ));
    }
    Ok(())
}

fn identity_stdout(batch: SshBatchOutput) -> ConnectorResult<Vec<u8>> {
    if batch.outcomes.len() != 1 {
        return Err(invalid_response());
    }
    match batch.outcomes.into_iter().next() {
        Some(ProbeOutcome::Success(output)) if output.probe_id == ProbeId::HostIdentityV1 => {
            Ok(output.stdout().to_vec())
        }
        Some(ProbeOutcome::Failure { failure, .. }) => Err(failure),
        _ => Err(invalid_response()),
    }
}

fn split_outcomes(
    batch: SshBatchOutput,
    planned: &[ProbeId],
) -> ConnectorResult<(ProbeOutputs, ModuleFailures)> {
    let mut outputs = BTreeMap::new();
    let mut failures = Vec::new();
    let mut seen = BTreeSet::new();
    for outcome in batch.outcomes {
        let probe_id = match &outcome {
            ProbeOutcome::Success(output) => output.probe_id,
            ProbeOutcome::Failure { probe_id, .. } => *probe_id,
        };
        if !planned.contains(&probe_id) || !seen.insert(probe_id) {
            return Err(invalid_response());
        }
        match outcome {
            ProbeOutcome::Success(output) => {
                outputs.insert(output.probe_id, output.stdout().to_vec());
            }
            ProbeOutcome::Failure { probe_id, failure } => {
                if matches!(
                    failure.code,
                    ErrorCode::HostKeyMismatch | ErrorCode::Cancelled
                ) {
                    return Err(failure);
                }
                failures.push((probe_id.as_str(), failure));
            }
        }
    }
    Ok((outputs, failures))
}

fn output(outputs: &BTreeMap<ProbeId, Vec<u8>>, id: ProbeId) -> Option<&[u8]> {
    outputs.get(&id).map(Vec::as_slice)
}

fn add_summary(summary: &mut ProviderRequestSummary, batch: &SshBatchOutput) {
    summary.request_count = summary
        .request_count
        .saturating_add(batch.outcomes.len() as u64);
    summary.elapsed_ms = summary.elapsed_ms.saturating_add(batch.elapsed_ms);
    for outcome in &batch.outcomes {
        let class = match outcome {
            ProbeOutcome::Success(_) => "success",
            ProbeOutcome::Failure { .. } => "failure",
        };
        *summary.status_class_counts.entry(class.into()).or_default() += 1;
    }
}

fn sort_and_validate(
    resources: &mut [ResourceObservation],
    relations: &mut [RelationObservation],
) -> ConnectorResult<()> {
    resources.sort_by_key(|resource| (resource.kind.clone(), resource.external_id.clone()));
    relations.sort_by_key(|relation| {
        (
            relation.source.kind.clone(),
            relation.source.external_id.clone(),
            relation.target.kind.clone(),
            relation.target.external_id.clone(),
            relation.kind.clone(),
            relation.evidence_key.clone(),
        )
    });
    if resources
        .iter()
        .map(|resource| (&resource.kind, &resource.external_id))
        .collect::<BTreeSet<_>>()
        .len()
        != resources.len()
        || relations
            .iter()
            .map(|relation| {
                (
                    &relation.source.kind,
                    &relation.source.external_id,
                    &relation.target.kind,
                    &relation.target.external_id,
                    &relation.kind,
                    &relation.evidence_key,
                )
            })
            .collect::<BTreeSet<_>>()
            .len()
            != relations.len()
    {
        return Err(invalid_response());
    }
    Ok(())
}

fn invalid_report(failure: ConnectorFailure) -> ValidationReport {
    ValidationReport {
        status: ValidationStatus::Invalid,
        warnings: Vec::new(),
        errors: vec![ValidationIssue {
            code: failure.code,
            message: failure.message,
        }],
    }
}

fn service_module(platform: HostPlatform) -> &'static str {
    match platform {
        HostPlatform::Darwin => "macos.launchd_services.v1",
        HostPlatform::Linux => "linux.systemd_services.v1",
    }
}

fn coverage_reason(code: ErrorCode) -> CoverageGapReason {
    match code {
        ErrorCode::PermissionDenied => CoverageGapReason::PermissionDenied,
        ErrorCode::ProviderUnavailable | ErrorCode::NetworkUnreachable => {
            CoverageGapReason::ProviderUnavailable
        }
        ErrorCode::InvalidResponse | ErrorCode::SchemaIncompatible => {
            CoverageGapReason::SchemaIncompatible
        }
        _ => CoverageGapReason::Other("ssh_module_partial".into()),
    }
}

fn invalid_config(message: &'static str) -> ConnectorFailure {
    ConnectorFailure {
        code: ErrorCode::InvalidDomainValue,
        message: message.into(),
        retryable: false,
        retry_after_ms: None,
    }
}

fn invalid_response() -> ConnectorFailure {
    ConnectorFailure {
        code: ErrorCode::InvalidResponse,
        message: "SSH probe response is invalid".into(),
        retryable: false,
        retry_after_ms: None,
    }
}

fn targeted_partial_failure() -> ConnectorFailure {
    ConnectorFailure {
        code: ErrorCode::PartialPagination,
        message: "SSH targeted sync cannot provide authoritative missing evidence".into(),
        retryable: false,
        retry_after_ms: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ProbeOutput;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    const IDENTITY: &str = "9f7fd5e6-3bc8-4daa-ae6b-9dfdffb54743";

    #[derive(Clone)]
    struct FakeClient {
        batches: Arc<Mutex<Vec<ConnectorResult<SshBatchOutput>>>>,
        requests: Arc<Mutex<Vec<Vec<ProbeId>>>>,
    }

    impl FakeClient {
        fn new(batches: Vec<ConnectorResult<SshBatchOutput>>) -> Self {
            Self {
                batches: Arc::new(Mutex::new(batches.into_iter().rev().collect())),
                requests: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    #[async_trait]
    impl SshProbeClient for FakeClient {
        async fn execute_batch(
            &self,
            _config: &SshConnectionConfigV1,
            probes: &[ProbeId],
            _cancellation: &SshCancellation,
        ) -> ConnectorResult<SshBatchOutput> {
            self.requests.lock().unwrap().push(probes.to_vec());
            self.batches.lock().unwrap().pop().unwrap()
        }
    }

    #[derive(Clone, Copy)]
    struct FixedClock;

    impl SshClock for FixedClock {
        fn now(&self) -> Timestamp {
            Timestamp::from_unix_millis(1_000).unwrap()
        }
    }

    fn success(id: ProbeId, stdout: &'static [u8]) -> ProbeOutcome {
        ProbeOutcome::Success(ProbeOutput::synthetic(id, stdout))
    }

    fn failed(id: ProbeId, code: ErrorCode) -> ProbeOutcome {
        ProbeOutcome::Failure {
            probe_id: id,
            failure: ConnectorFailure {
                code,
                message: "SSH probe failed".into(),
                retryable: true,
                retry_after_ms: None,
            },
        }
    }

    fn batch(outcomes: Vec<ProbeOutcome>) -> ConnectorResult<SshBatchOutput> {
        Ok(SshBatchOutput {
            outcomes,
            elapsed_ms: 5,
            output_bytes: 100,
        })
    }

    fn identity(platform: &'static [u8]) -> ConnectorResult<SshBatchOutput> {
        batch(vec![success(ProbeId::HostIdentityV1, platform)])
    }

    fn request(platform_service: &str) -> SyncRequest {
        SyncRequest {
            sync_run_id: SyncRunId::new("ssh-fixture-run").unwrap(),
            connection: ConnectionInput {
                connection_id: ConnectionId::new("ssh-fixture-connection").unwrap(),
                connector_type: ConnectorType::new("ssh").unwrap(),
                config: json!({
                    "host_identity": IDENTITY,
                    "host_alias": "fixture-host",
                    "connect_timeout_secs": 10,
                    "probe_profile": "baseline-v1",
                    "allowed_service_ids": [platform_service],
                }),
                config_schema_version: SchemaVersion::new(1).unwrap(),
            },
            mode: SyncMode::Full,
            scope: Scope::new("ssh-fixture-scope").unwrap(),
            cursor: None,
            targeted_resources: Vec::new(),
        }
    }

    fn common_outcomes(service: ProbeOutcome) -> ConnectorResult<SshBatchOutput> {
        batch(vec![
            success(ProbeId::HostUptimeV1, b"12:00 up 2 days, 1 user"),
            success(
                ProbeId::HostFilesystemsV1,
                b"Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/a 10 1 9 10% /a\n",
            ),
            success(ProbeId::HostProcessSummaryV1, b"R process\nS process\n"),
            service,
        ])
    }

    #[tokio::test]
    async fn darwin_full_sync_selects_launchd_and_is_authoritative() {
        let fake = FakeClient::new(vec![
            identity(b"Darwin\narm64\n"),
            common_outcomes(success(
                ProbeId::MacosLaunchdServicesV1,
                b"PID Status Label\n123 0 app.service\n",
            )),
        ]);
        let requests = fake.requests.clone();
        let connector = SshConnector::with_clock(fake, FixedClock);
        let outcome = connector.sync(request("app.service"), None).await.unwrap();
        let SyncOutcome::Complete { batch } = outcome else {
            panic!("all successful SSH probes must be complete")
        };
        assert!(matches!(
            batch.coverage,
            SyncCoverage::AuthoritativeFull { .. }
        ));
        assert_eq!(batch.resources.len(), 4);
        assert_eq!(batch.relations.len(), 3);
        assert_eq!(batch.provider_request_summary.request_count, 5);
        assert_eq!(
            requests.lock().unwrap()[1],
            vec![
                ProbeId::HostUptimeV1,
                ProbeId::HostFilesystemsV1,
                ProbeId::HostProcessSummaryV1,
                ProbeId::MacosLaunchdServicesV1,
            ]
        );
    }

    #[tokio::test]
    async fn linux_child_failure_is_partial_and_common_resources_survive() {
        let fake = FakeClient::new(vec![
            identity(b"Linux\nx86_64\n"),
            common_outcomes(failed(
                ProbeId::LinuxSystemdServicesV1,
                ErrorCode::ProviderUnavailable,
            )),
        ]);
        let connector = SshConnector::with_clock(fake, FixedClock);
        let outcome = connector.sync(request("app.service"), None).await.unwrap();
        let SyncOutcome::Partial { batch, failure } = outcome else {
            panic!("service failure must be partial")
        };
        assert_eq!(failure.code, ErrorCode::ProviderUnavailable);
        assert_eq!(batch.resources.len(), 3);
        assert_eq!(batch.relations.len(), 2);
        assert!(
            !batch
                .resources
                .iter()
                .any(|resource| resource.health == ResourceHealth::Unhealthy)
        );
    }

    #[tokio::test]
    async fn late_host_key_failure_discards_staged_identity() {
        let failure = ConnectorFailure {
            code: ErrorCode::HostKeyMismatch,
            message: "SSH host key verification failed".into(),
            retryable: false,
            retry_after_ms: None,
        };
        let fake = FakeClient::new(vec![identity(b"Linux\nx86_64\n"), Err(failure)]);
        let connector = SshConnector::with_clock(fake, FixedClock);
        let failure = connector
            .sync(request("app.service"), None)
            .await
            .unwrap_err();
        assert_eq!(failure.code, ErrorCode::HostKeyMismatch);
    }

    #[tokio::test]
    async fn in_band_host_key_failure_is_also_fatal() {
        let fake = FakeClient::new(vec![
            identity(b"Linux\nx86_64\n"),
            batch(vec![failed(
                ProbeId::HostUptimeV1,
                ErrorCode::HostKeyMismatch,
            )]),
        ]);
        let connector = SshConnector::with_clock(fake, FixedClock);
        let failure = connector
            .sync(request("app.service"), None)
            .await
            .unwrap_err();
        assert_eq!(failure.code, ErrorCode::HostKeyMismatch);
    }

    #[tokio::test]
    async fn targeted_sync_is_partial_without_fabricating_resource_ids() {
        let fake = FakeClient::new(vec![
            identity(b"Linux\nx86_64\n"),
            common_outcomes(success(
                ProbeId::LinuxSystemdServicesV1,
                b"app.service loaded active running fixture\n",
            )),
        ]);
        let connector = SshConnector::with_clock(fake, FixedClock);
        let mut request = request("app.service");
        request.mode = SyncMode::Targeted;
        request.targeted_resources = vec![ResourceLocator {
            kind: ResourceKind::new("ssh.host").unwrap(),
            external_id: ExternalId::new(format!("ssh-host:v1:{IDENTITY}")).unwrap(),
        }];
        let outcome = connector.sync(request, None).await.unwrap();
        let SyncOutcome::Partial { batch, failure } = outcome else {
            panic!("targeted SSH coverage must remain partial")
        };
        assert_eq!(failure.code, ErrorCode::PartialPagination);
        assert!(matches!(batch.coverage, SyncCoverage::Partial { .. }));
    }
}
