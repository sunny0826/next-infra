use next_infra_connector_api::*;
use next_infra_core::*;
use serde_json::json;

fn schema() -> SchemaVersion {
    SchemaVersion::new(1).unwrap()
}

fn connection() -> ConnectionInput {
    ConnectionInput {
        connection_id: ConnectionId::new("fixture-connection").unwrap(),
        connector_type: ConnectorType::new("fixture").unwrap(),
        config: json!({"endpoint": "https://example.test"}),
        config_schema_version: schema(),
    }
}

fn full_request() -> SyncRequest {
    SyncRequest {
        sync_run_id: SyncRunId::new("fixture-run").unwrap(),
        connection: connection(),
        mode: SyncMode::Full,
        scope: Scope::new("fixture-scope").unwrap(),
        cursor: None,
        targeted_resources: Vec::new(),
    }
}

#[test]
fn serializable_requests_do_not_have_secret_fields() {
    let serialized = serde_json::to_value(full_request()).unwrap();
    let text = serialized.to_string().to_lowercase();

    for forbidden in ["secret", "credential", "token", "password"] {
        assert!(
            !text.contains(forbidden),
            "serialized request leaked {forbidden}"
        );
    }
}

#[test]
fn partial_and_fatal_results_are_structurally_distinct() {
    fn classify(result: ConnectorResult<SyncOutcome>) -> &'static str {
        match result {
            Ok(SyncOutcome::Complete { .. }) => "complete",
            Ok(SyncOutcome::Partial { .. }) => "partial",
            Err(_) => "fatal",
        }
    }

    let failure = ConnectorFailure {
        code: ErrorCode::PartialPagination,
        message: "fixture pagination stopped".into(),
        retryable: true,
        retry_after_ms: None,
    };
    let batch = ObservationBatch {
        resources: Vec::new(),
        relations: Vec::new(),
        coverage: SyncCoverage::Partial {
            scope: Some(Scope::new("fixture-scope").unwrap()),
            reason: CoverageGapReason::PaginationIncomplete,
        },
        next_cursor: None,
        warnings: Vec::new(),
        redaction_report: RedactionReport::default(),
        provider_request_summary: ProviderRequestSummary::default(),
    };

    assert_eq!(
        classify(Ok(SyncOutcome::Partial {
            batch,
            failure: failure.clone(),
        })),
        "partial"
    );
    assert_eq!(classify(Err(failure)), "fatal");
}

#[test]
fn sync_mode_only_accepts_compatible_coverage() {
    let request = full_request();
    assert!(request.accepts_coverage(&SyncCoverage::AuthoritativeFull {
        scope: Scope::new("fixture-scope").unwrap(),
    }));
    assert!(!request.accepts_coverage(&SyncCoverage::Incremental {
        cursor: SyncCursor::new("fixture-cursor").unwrap(),
    }));
}

#[test]
fn descriptor_rejects_duplicate_resource_kinds() {
    let kind = ResourceKind::new("fixture.resource").unwrap();
    let capability = ResourceCapability {
        kind: kind.clone(),
        attribute_schema_version: schema(),
        coverage: ConnectorCoverage {
            module: "resources".into(),
            level: ConnectorCoverageLevel::Supported,
            reason: None,
        },
    };
    let descriptor = ConnectorDescriptor {
        connector_type: ConnectorType::new("fixture").unwrap(),
        connector_version: "1.0.0".into(),
        config_schema_version: schema(),
        auth: AuthDescriptor {
            kind: AuthKind::None,
            minimum_permissions: Vec::new(),
        },
        sync_modes: vec![SyncMode::Full],
        resources: vec![capability.clone(), capability],
        relations: Vec::new(),
        sensitive_field_policy: Vec::new(),
        rate_limit: RateLimitGuidance {
            default_max_concurrency: 1,
            requests_per_minute: None,
            respects_retry_after: true,
        },
        recommended_sync_interval_secs: 60,
        known_gaps: Vec::new(),
    };

    assert!(descriptor.validate().is_err());
}

#[test]
fn reports_and_outcomes_reject_incoherent_states() {
    let invalid_report = ValidationReport {
        status: ValidationStatus::Valid,
        warnings: Vec::new(),
        errors: vec![ValidationIssue {
            code: ErrorCode::PermissionDenied,
            message: "fixture permission".into(),
        }],
    };
    assert!(invalid_report.validate().is_err());

    let batch = ObservationBatch {
        resources: Vec::new(),
        relations: Vec::new(),
        coverage: SyncCoverage::Partial {
            scope: Some(Scope::new("fixture-scope").unwrap()),
            reason: CoverageGapReason::PaginationIncomplete,
        },
        next_cursor: None,
        warnings: Vec::new(),
        redaction_report: RedactionReport::default(),
        provider_request_summary: ProviderRequestSummary::default(),
    };
    let outcome = SyncOutcome::Complete { batch };
    assert!(outcome.validate_for(&full_request()).is_err());
}

#[test]
fn read_connector_is_object_safe_for_registry_use() {
    let connector: Option<&dyn ReadConnector> = None;
    assert!(connector.is_none());
}
