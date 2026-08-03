//! Deterministic, fully offline connector fixture for Next Infra.

use async_trait::async_trait;
use next_infra_connector_api::*;
use next_infra_core::*;
use serde::{Deserialize, Serialize};

const STANDARD_REPLAY: &str = include_str!("../../../fixtures/connectors/fixture/replay-v1.json");

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixturePlan {
    pub steps: Vec<FixtureStep>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixtureStep {
    pub mode: SyncMode,
    pub cursor_before: Option<SyncCursor>,
    pub result: FixtureResult,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FixtureResult {
    Complete {
        batch: ObservationBatch,
    },
    Partial {
        batch: ObservationBatch,
        failure: ConnectorFailure,
    },
    Fatal {
        failure: ConnectorFailure,
    },
}

pub struct FixtureConnector {
    descriptor: ConnectorDescriptor,
    plan: FixturePlan,
}

impl FixtureConnector {
    pub fn standard() -> ConnectorResult<Self> {
        Self::from_json(STANDARD_REPLAY)
    }

    pub fn from_json(input: &str) -> ConnectorResult<Self> {
        let plan: FixturePlan = serde_json::from_str(input).map_err(|error| ConnectorFailure {
            code: ErrorCode::InvalidResponse,
            message: format!("invalid fixture replay: {error}"),
            retryable: false,
            retry_after_ms: None,
        })?;
        let descriptor = fixture_descriptor();
        descriptor.validate().map_err(|error| ConnectorFailure {
            code: error.code,
            message: error.message,
            retryable: error.retryable,
            retry_after_ms: None,
        })?;
        if plan.steps.is_empty() {
            return Err(ConnectorFailure {
                code: ErrorCode::InvalidResponse,
                message: "fixture replay must contain at least one step".into(),
                retryable: false,
                retry_after_ms: None,
            });
        }
        Ok(Self { descriptor, plan })
    }

    pub fn replay(&self, request: &SyncRequest) -> ConnectorResult<SyncOutcome> {
        let step = self
            .plan
            .steps
            .iter()
            .find(|step| step.mode == request.mode && step.cursor_before == request.cursor)
            .ok_or_else(|| ConnectorFailure {
                code: ErrorCode::InvalidResponse,
                message: "fixture replay has no matching mode/cursor step".into(),
                retryable: false,
                retry_after_ms: None,
            })?;
        let outcome = match &step.result {
            FixtureResult::Complete { batch } => SyncOutcome::Complete {
                batch: batch.clone(),
            },
            FixtureResult::Partial { batch, failure } => SyncOutcome::Partial {
                batch: batch.clone(),
                failure: failure.clone(),
            },
            FixtureResult::Fatal { failure } => return Err(failure.clone()),
        };
        outcome
            .validate_for(request)
            .map_err(|error| ConnectorFailure {
                code: error.code,
                message: error.message,
                retryable: error.retryable,
                retry_after_ms: None,
            })?;
        Ok(outcome)
    }
}

#[async_trait]
impl ReadConnector for FixtureConnector {
    fn descriptor(&self) -> &ConnectorDescriptor {
        &self.descriptor
    }

    async fn validate(
        &self,
        request: ValidationRequest,
        secret: Option<&SecretValue>,
    ) -> ConnectorResult<ValidationReport> {
        let mut errors = Vec::new();
        if request.connection.connector_type != self.descriptor.connector_type {
            errors.push(ValidationIssue {
                code: ErrorCode::InvalidDomainValue,
                message: "fixture connection uses a different connector type".into(),
            });
        }
        if secret.is_some() {
            errors.push(ValidationIssue {
                code: ErrorCode::InvalidDomainValue,
                message: "fixture connector does not accept a secret".into(),
            });
        }
        Ok(ValidationReport {
            status: if errors.is_empty() {
                ValidationStatus::Valid
            } else {
                ValidationStatus::Invalid
            },
            warnings: Vec::new(),
            errors,
        })
    }

    async fn sync(
        &self,
        request: SyncRequest,
        secret: Option<&SecretValue>,
    ) -> ConnectorResult<SyncOutcome> {
        if secret.is_some() {
            return Err(ConnectorFailure {
                code: ErrorCode::InvalidDomainValue,
                message: "fixture connector does not accept a secret".into(),
                retryable: false,
                retry_after_ms: None,
            });
        }
        if request.connection.connector_type != self.descriptor.connector_type {
            return Err(ConnectorFailure {
                code: ErrorCode::InvalidDomainValue,
                message: "fixture connection uses a different connector type".into(),
                retryable: false,
                retry_after_ms: None,
            });
        }
        self.replay(&request)
    }
}

fn fixture_descriptor() -> ConnectorDescriptor {
    let resource_kind = ResourceKind::new("fixture.resource").expect("static fixture kind");
    ConnectorDescriptor {
        connector_type: ConnectorType::new("fixture").expect("static fixture connector type"),
        connector_version: "1.0.0".into(),
        config_schema_version: SchemaVersion::new(1).expect("static schema version"),
        auth: AuthDescriptor {
            kind: AuthKind::None,
            minimum_permissions: Vec::new(),
        },
        sync_modes: vec![SyncMode::Full, SyncMode::Incremental, SyncMode::Targeted],
        resources: vec![ResourceCapability {
            kind: resource_kind.clone(),
            attribute_schema_version: SchemaVersion::new(1).expect("static schema version"),
            coverage: ConnectorCoverage {
                module: "fixture.resources".into(),
                level: ConnectorCoverageLevel::Supported,
                reason: None,
            },
        }],
        relations: vec![RelationCapability {
            kind: RelationKind::new("fixture.depends_on").expect("static relation kind"),
            source_kind: resource_kind.clone(),
            target_kind: resource_kind,
            coverage: ConnectorCoverage {
                module: "fixture.relations".into(),
                level: ConnectorCoverageLevel::Supported,
                reason: None,
            },
        }],
        sensitive_field_policy: vec!["fixture replay contains no secrets".into()],
        rate_limit: RateLimitGuidance {
            default_max_concurrency: 1,
            requests_per_minute: None,
            respects_retry_after: true,
        },
        recommended_sync_interval_secs: 60,
        known_gaps: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request(mode: SyncMode, cursor: Option<&str>) -> SyncRequest {
        SyncRequest {
            sync_run_id: SyncRunId::new("fixture-run").unwrap(),
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
    fn replay_is_deterministic_for_same_request() {
        let connector = FixtureConnector::standard().unwrap();
        let first = connector.replay(&request(SyncMode::Full, None)).unwrap();
        let second = connector.replay(&request(SyncMode::Full, None)).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn replay_covers_full_incremental_targeted_partial_and_fatal() {
        let connector = FixtureConnector::standard().unwrap();
        assert!(matches!(
            connector.replay(&request(SyncMode::Full, None)),
            Ok(SyncOutcome::Complete { .. })
        ));
        assert!(matches!(
            connector.replay(&request(SyncMode::Incremental, Some("cursor-v1"))),
            Ok(SyncOutcome::Complete { .. })
        ));
        assert!(matches!(
            connector.replay(&request(SyncMode::Targeted, None)),
            Ok(SyncOutcome::Complete { .. })
        ));
        assert!(matches!(
            connector.replay(&request(SyncMode::Full, Some("cursor-partial"))),
            Ok(SyncOutcome::Partial { .. })
        ));
        assert!(
            connector
                .replay(&request(SyncMode::Full, Some("cursor-fatal")))
                .is_err()
        );
    }

    #[test]
    fn fixture_payload_contains_only_synthetic_identifiers() {
        let lower = STANDARD_REPLAY.to_ascii_lowercase();
        for forbidden in ["github.com", "10.0.", "192.168.", "bearer ", "password"] {
            assert!(!lower.contains(forbidden), "fixture contains {forbidden}");
        }
        assert!(lower.contains("fixture-"));
        assert!(!lower.contains("http://"));
        assert!(!lower.contains("https://"));
    }
}
