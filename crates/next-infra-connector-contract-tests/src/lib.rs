//! Reusable connector conformance checks for Next Infra.

use next_infra_connector_api::{ConnectorDescriptor, ObservationBatch, SyncOutcome, SyncRequest};
use next_infra_connector_catalog::ConnectorCoverageSnapshot;
use next_infra_core::ErrorCode;
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConformanceIssue {
    pub code: ErrorCode,
    pub message: String,
}

pub fn check_descriptor(descriptor: &ConnectorDescriptor) -> Vec<ConformanceIssue> {
    let mut issues = Vec::new();
    if let Err(error) = descriptor.validate() {
        issues.push(ConformanceIssue {
            code: error.code,
            message: error.message,
        });
        return issues;
    }
    if ConnectorCoverageSnapshot::from_descriptor(descriptor).is_err() {
        issues.push(issue(
            "descriptor cannot be projected into the coverage catalog",
        ));
    }
    for coverage in descriptor
        .resources
        .iter()
        .map(|capability| &capability.coverage)
        .chain(
            descriptor
                .relations
                .iter()
                .map(|capability| &capability.coverage),
        )
    {
        let needs_reason = !matches!(
            coverage.level,
            next_infra_core::ConnectorCoverageLevel::Supported
        );
        if needs_reason != coverage.reason.is_some() {
            issues.push(issue(
                "partial/unsupported coverage requires a reason and supported coverage forbids one",
            ));
        }
    }
    issues
}

pub fn check_outcome(request: &SyncRequest, outcome: &SyncOutcome) -> Vec<ConformanceIssue> {
    let mut issues = Vec::new();
    if let Err(error) = outcome.validate_for(request) {
        issues.push(ConformanceIssue {
            code: error.code,
            message: error.message,
        });
        return issues;
    }
    issues.extend(check_batch(outcome.batch()));
    issues
}

pub fn check_batch(batch: &ObservationBatch) -> Vec<ConformanceIssue> {
    let mut issues = Vec::new();
    if batch.redaction_report.secret_sentinels_detected != 0 {
        issues.push(issue("redaction report contains secret sentinels"));
    }
    let serialized = serde_json::to_string(batch)
        .expect("ObservationBatch serialization is part of the connector contract")
        .to_ascii_lowercase();
    for forbidden in ["password", "private_key", "authorization", "bearer "] {
        if serialized.contains(forbidden) {
            issues.push(issue(format!("serialized batch contains {forbidden}")));
        }
    }

    let resource_keys = batch
        .resources
        .iter()
        .map(|resource| (resource.kind.clone(), resource.external_id.clone()))
        .collect::<Vec<_>>();
    if resource_keys.windows(2).any(|pair| pair[0] > pair[1]) {
        issues.push(issue("resources are not in deterministic identity order"));
    }
    if resource_keys.iter().collect::<BTreeSet<_>>().len() != resource_keys.len() {
        issues.push(issue("resources contain duplicate identities"));
    }

    let relation_keys = batch
        .relations
        .iter()
        .map(|relation| {
            (
                relation.source.kind.clone(),
                relation.source.external_id.clone(),
                relation.target.kind.clone(),
                relation.target.external_id.clone(),
                relation.kind.clone(),
                relation.evidence_key.clone(),
            )
        })
        .collect::<Vec<_>>();
    if relation_keys.windows(2).any(|pair| pair[0] > pair[1]) {
        issues.push(issue("relations are not in deterministic evidence order"));
    }
    if relation_keys.iter().collect::<BTreeSet<_>>().len() != relation_keys.len() {
        issues.push(issue("relations contain duplicate evidence identities"));
    }
    issues
}

fn issue(message: impl Into<String>) -> ConformanceIssue {
    ConformanceIssue {
        code: ErrorCode::InvalidResponse,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use next_infra_connector_api::*;
    use next_infra_connector_fixture::FixtureConnector;
    use next_infra_core::*;
    use serde_json::json;

    fn request(mode: SyncMode, cursor: Option<&str>) -> SyncRequest {
        SyncRequest {
            sync_run_id: SyncRunId::new("fixture-contract-run").unwrap(),
            connection: ConnectionInput {
                connection_id: ConnectionId::new("fixture-connection").unwrap(),
                connector_type: ConnectorType::new("fixture").unwrap(),
                config: json!({}),
                config_schema_version: SchemaVersion::new(1).unwrap(),
            },
            mode,
            scope: Scope::new("fixture-scope").unwrap(),
            cursor: cursor.map(|value| SyncCursor::new(value).unwrap()),
            targeted_resources: Vec::new(),
        }
    }

    #[test]
    fn fixture_descriptor_and_replays_pass_common_conformance() {
        let connector = FixtureConnector::standard().unwrap();
        assert!(check_descriptor(connector.descriptor()).is_empty());
        for request in [
            request(SyncMode::Full, None),
            request(SyncMode::Incremental, Some("cursor-v1")),
            request(SyncMode::Targeted, None),
            request(SyncMode::Full, Some("cursor-partial")),
        ] {
            let outcome = connector.replay(&request).unwrap();
            assert_eq!(check_outcome(&request, &outcome), Vec::new());
        }
    }

    #[test]
    fn conformance_rejects_unsorted_duplicate_resources() {
        let connector = FixtureConnector::standard().unwrap();
        let request = request(SyncMode::Full, None);
        let mut outcome = connector.replay(&request).unwrap();
        let batch = match &mut outcome {
            SyncOutcome::Complete { batch } | SyncOutcome::Partial { batch, .. } => batch,
        };
        batch.resources.swap(0, 1);
        batch.resources.push(batch.resources[0].clone());

        let issues = check_outcome(&request, &outcome);
        assert!(issues.len() >= 2);
    }
}
