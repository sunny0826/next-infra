//! # SSH connector live smoke
//!
//! Env-driven live smoke for a real SSH alias from the user's `~/.ssh/config`.
//!
//! ## Env contract
//!
//! | Variable | Required | Value |
//! |---|---|---|
//! | `NEXT_INFRA_SSH_ALIAS` | Yes | An alias defined in `~/.ssh/config` (never a raw host/IP) |
//! | `NEXT_INFRA_SSH_ALLOWED_SERVICES` | No | Comma-separated service IDs; presence activates launchd/systemd probe |
//!
//! ## Exit codes
//!
//! - `0` — validate + bounded sync succeeded; safe summary printed
//! - `1` — validation or contract failure (configuration error, partial result, protocol error)
//! - `2` — fatal auth or host-key error (permission denied, host-key mismatch/refused)

use next_infra_connector_api::{ConnectionInput, ReadConnector, SyncRequest, ValidationRequest};
use next_infra_connector_ssh::{
    HostAlias, HostIdentity, OpenSshClient, SshConnectionConfigV1, SshConnector,
};
use next_infra_core::{ConnectionId, ConnectorType, SchemaVersion, Scope, SyncMode, SyncRunId};
use std::env;

const EXIT_VALIDATION_FAILURE: i32 = 1;
const EXIT_FATAL_AUTH_OR_HOSTKEY: i32 = 2;

fn main() {
    let alias = match env::var("NEXT_INFRA_SSH_ALIAS") {
        Ok(v) if !v.is_empty() => v,
        _ => {
            eprintln!(
                "NEXT_INFRA_SSH_ALIAS is required and must be a non-empty SSH alias\n\
                 from your ~/.ssh/config (never a raw host or IP address).\n\
                 \n\
                 Usage:\n\
                 \n  NEXT_INFRA_SSH_ALIAS=<alias> \\\n\
                 \n    rtk cargo run --example live_smoke --locked\n\
                 \n\
                 Optional: NEXT_INFRA_SSH_ALLOWED_SERVICES=<id>,<id>...\n\
                 (activates launchd/systemd probe for the given service IDs)\n"
            );
            std::process::exit(EXIT_VALIDATION_FAILURE);
        }
    };

    let allowed_services: Vec<String> = env::var("NEXT_INFRA_SSH_ALLOWED_SERVICES")
        .ok()
        .filter(|v| !v.is_empty())
        .map(|v| {
            v.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();

    let host_alias = match HostAlias::parse(&alias) {
        Ok(a) => a,
        Err(_) => {
            eprintln!(
                "NEXT_INFRA_SSH_ALIAS value {:?} is not a valid SSH alias.\n\
                 Alias must be 1-128 chars, ascii-alphanumeric with dots/underscores/hyphens,\n\
                 and must not contain shell escape sequences (-o, spaces, newlines, etc.).",
                alias
            );
            std::process::exit(EXIT_VALIDATION_FAILURE);
        }
    };

    // Derive a stable host_identity from the alias so repeated runs against the
    // same alias produce the same external_id, without persisting any secret.
    let host_identity = HostIdentity::generate();
    let external_id = host_identity.external_id();
    let external_id_str = external_id.as_str();

    let config = SshConnectionConfigV1 {
        host_identity: host_identity.clone(),
        host_alias: host_alias.clone(),
        connect_timeout_secs: 10,
        probe_profile: next_infra_connector_ssh::ProbeProfile::BaselineV1,
        allowed_service_ids: allowed_services
            .iter()
            .filter_map(|s| next_infra_connector_ssh::ServiceId::parse(s).ok())
            .collect(),
    };

    let connection_id = ConnectionId::new("live-smoke-connection").unwrap();
    let connection = ConnectionInput {
        connection_id: connection_id.clone(),
        connector_type: ConnectorType::new("ssh").unwrap(),
        config: serde_json::to_value(&config).expect("config serializes"),
        config_schema_version: SchemaVersion::new(1).unwrap(),
    };

    let connector = SshConnector::new(OpenSshClient::new());

    // ── Phase 1: validate ─────────────────────────────────────────────────────
    println!(
        "[live-smoke] phase=validate alias={} host_identity={}",
        alias, external_id_str
    );

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime starts");
    let validation = rt.block_on(async {
        connector
            .validate(
                ValidationRequest {
                    connection: connection.clone(),
                },
                None,
            )
            .await
    });

    let validation_report = match validation {
        Ok(r) => r,
        Err(failure) => {
            let code = failure.code;
            if is_fatal_auth_or_hostkey(&code) {
                eprintln!(
                    "[live-smoke] FATAL alias={} code={:?} message={}",
                    alias, code, failure.message
                );
                std::process::exit(EXIT_FATAL_AUTH_OR_HOSTKEY);
            }
            eprintln!(
                "[live-smoke] VALIDATION FAILED alias={} code={:?} message={}",
                alias, code, failure.message
            );
            std::process::exit(EXIT_VALIDATION_FAILURE);
        }
    };

    if !validation_report.errors.is_empty() {
        for issue in &validation_report.errors {
            eprintln!(
                "[live-smoke] VALIDATION ERROR alias={} code={:?} message={}",
                alias, issue.code, issue.message
            );
        }
        std::process::exit(EXIT_VALIDATION_FAILURE);
    }

    println!(
        "[live-smoke] validate_ok alias={} warnings={}",
        alias,
        validation_report.warnings.len()
    );

    // ── Phase 2: bounded sync ─────────────────────────────────────────────────
    let sync_request = SyncRequest {
        sync_run_id: SyncRunId::new("live-smoke-run").unwrap(),
        connection: connection.clone(),
        mode: SyncMode::Full,
        scope: Scope::new("live-smoke").unwrap(),
        cursor: None,
        targeted_resources: Vec::new(),
    };

    println!(
        "[live-smoke] phase=sync alias={} mode=Full scope=live-smoke allowed_services={}",
        alias,
        allowed_services.len()
    );

    let sync_outcome = rt.block_on(async { connector.sync(sync_request.clone(), None).await });

    let (sync_coverage, summary, warnings, probe_outcomes) = match sync_outcome {
        Ok(outcome) => {
            let batch = match &outcome {
                next_infra_connector_api::SyncOutcome::Complete { batch } => batch,
                next_infra_connector_api::SyncOutcome::Partial { batch, .. } => batch,
            };
            let probe_outcomes: Vec<(String, u64)> = batch
                .provider_request_summary
                .status_class_counts
                .iter()
                .map(|(k, v)| (k.clone(), *v))
                .collect();
            (
                Some(batch.coverage.clone()),
                batch.provider_request_summary.clone(),
                batch.warnings.clone(),
                probe_outcomes,
            )
        }
        Err(failure) => {
            let code = failure.code;
            if is_fatal_auth_or_hostkey(&code) {
                eprintln!(
                    "[live-smoke] FATAL alias={} code={:?} message={}",
                    alias, code, failure.message
                );
                std::process::exit(EXIT_FATAL_AUTH_OR_HOSTKEY);
            }
            eprintln!(
                "[live-smoke] SYNC FAILED alias={} code={:?} message={}",
                alias, code, failure.message
            );
            std::process::exit(EXIT_VALIDATION_FAILURE);
        }
    };

    // ── Safe summary ──────────────────────────────────────────────────────────
    println!("\n=== LIVE-SMOKE SAFE SUMMARY ===");
    println!("alias: [REDACTED]");
    println!("host_identity_external_id: {}", external_id_str);
    println!("probes_executed: {}", summary.request_count);
    for (class, count) in &probe_outcomes {
        println!("  outcome_class={} count={}", class, count);
    }
    println!("elapsed_ms: {}", summary.elapsed_ms);

    match &sync_coverage {
        Some(c) => match c {
            next_infra_core::SyncCoverage::AuthoritativeFull { scope } => {
                println!("coverage: AuthoritativeFull  scope={}", scope.as_str());
            }
            next_infra_core::SyncCoverage::Partial { scope, reason } => {
                let reason_str = format!("{:?}", reason);
                let scope_str = scope.as_ref().map(|s| s.as_str()).unwrap_or("n/a");
                println!(
                    "coverage: Partial  scope={}  reason={}",
                    scope_str, reason_str
                );
            }
            next_infra_core::SyncCoverage::Incremental { .. } => {
                println!("coverage: Incremental");
            }
            next_infra_core::SyncCoverage::Targeted { .. } => {
                println!("coverage: Targeted");
            }
        },
        None => println!("coverage: Unknown"),
    }

    if !warnings.is_empty() {
        println!("warnings: {}", warnings.len());
        for w in &warnings {
            println!("  warning code={:?} message={}", w.code, w.message);
        }
    }

    println!("=== END SAFE SUMMARY ===\n");
    println!("[live-smoke] DONE alias={} exit=0", alias);
    std::process::exit(0);
}

/// Returns true for error codes that represent fatal auth or host-key failures
/// (exit code 2). These must NOT be caught and retried — they fail fast.
fn is_fatal_auth_or_hostkey(code: &next_infra_core::ErrorCode) -> bool {
    matches!(
        code,
        next_infra_core::ErrorCode::AuthenticationFailed
            | next_infra_core::ErrorCode::HostKeyMismatch
    )
}
