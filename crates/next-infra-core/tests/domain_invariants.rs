use next_infra_core::{
    BindingId, BindingStatus, ChangeSubject, Confidence, ConnectionId, ConnectorHealth,
    ConnectorType, EvidenceType, FieldPath, Freshness, Lifecycle, OriginRef, RelationEvidence,
    RelationKind, RelationVersionId, ResourceHealth, ResourceId, ResourceKind, ResourceVersionId,
    RuleVersion, Scope, SecretBackend, SecretKind, SecretRef, SecretRefInput, SecretValue,
    SyncCoverage, SyncCursor, SyncRunId, Timestamp,
};
use std::any::TypeId;

fn id<T>(
    value: &str,
    constructor: impl FnOnce(String) -> Result<T, next_infra_core::DomainError>,
) -> T {
    constructor(value.to_owned()).expect("fixture identifier must be valid")
}

#[test]
fn connector_and_domain_kinds_enforce_lowercase_shapes() {
    assert_eq!(ConnectorType::new("github").unwrap().as_str(), "github");
    assert!(ConnectorType::new("GitHub").is_err());
    assert!(ConnectorType::new("github.api").is_err());

    assert_eq!(
        ResourceKind::new("github.repository").unwrap().as_str(),
        "github.repository"
    );
    assert_eq!(
        RelationKind::new("deployment.runs_on").unwrap().as_str(),
        "deployment.runs_on"
    );

    for invalid in [
        "repository",
        "GitHub.repository",
        "github.Repository",
        "github.",
        ".repository",
    ] {
        assert!(ResourceKind::new(invalid).is_err(), "accepted {invalid}");
        assert!(RelationKind::new(invalid).is_err(), "accepted {invalid}");
    }
}

#[test]
fn only_authoritative_full_coverage_contributes_missing_evidence() {
    let authoritative = SyncCoverage::AuthoritativeFull {
        scope: id("fixture-scope", Scope::new),
    };
    let incremental = SyncCoverage::Incremental {
        cursor: id("fixture-cursor", SyncCursor::new),
    };
    let partial = SyncCoverage::Partial {
        scope: Some(id("fixture-scope", Scope::new)),
        reason: next_infra_core::CoverageGapReason::PaginationIncomplete,
    };
    let targeted = SyncCoverage::Targeted {
        resource_ids: vec![id("fixture-resource", ResourceId::new)],
    };

    assert!(authoritative.contributes_missing_evidence());
    assert!(!incremental.contributes_missing_evidence());
    assert!(!partial.contributes_missing_evidence());
    assert!(!targeted.contributes_missing_evidence());
}

#[test]
fn relation_evidence_keeps_provenance_and_confidence_distinct() {
    let provider_sync_run = id("fixture-sync-run", SyncRunId::new);
    let provider = RelationEvidence::Provider {
        connection_id: id("fixture-connection", ConnectionId::new),
        sync_run_id: provider_sync_run.clone(),
        field_path: id("attributes.fixture", FieldPath::new),
    };
    let configured = RelationEvidence::Configured {
        binding_id: id("fixture-binding", BindingId::new),
    };
    let confidence = Confidence::from_basis_points(8_500).unwrap();
    let inferred = RelationEvidence::Inferred {
        rule_version: id("fixture-rule-v1", RuleVersion::new),
        input_resource_version_ids: vec![id("fixture-resource-version", ResourceVersionId::new)],
        input_relation_version_ids: vec![id("fixture-relation-version", RelationVersionId::new)],
        confidence,
    };

    assert_eq!(provider.evidence_type(), EvidenceType::Provider);
    assert_eq!(configured.evidence_type(), EvidenceType::Configured);
    assert_eq!(inferred.evidence_type(), EvidenceType::Inferred);

    assert_eq!(provider.sync_run_id(), Some(&provider_sync_run));
    assert_eq!(configured.sync_run_id(), None);
    assert_eq!(inferred.sync_run_id(), None);

    assert_eq!(provider.confidence(), None);
    assert_eq!(configured.confidence(), None);
    assert_eq!(inferred.confidence(), Some(confidence));

    let serialized = serde_json::to_value(&inferred).unwrap();
    assert_eq!(
        serialized["input_relation_version_ids"],
        serde_json::json!(["fixture-relation-version"])
    );
}

#[test]
fn inferred_relation_evidence_defaults_relation_inputs_for_legacy_payloads() {
    let legacy = serde_json::json!({
        "type": "inferred",
        "rule_version": "fixture-rule-v1",
        "input_resource_version_ids": ["fixture-resource-version"],
        "confidence": 8500,
    });

    let evidence: RelationEvidence = serde_json::from_value(legacy).unwrap();
    match evidence {
        RelationEvidence::Inferred {
            input_resource_version_ids,
            input_relation_version_ids,
            ..
        } => {
            assert_eq!(
                input_resource_version_ids,
                vec![id("fixture-resource-version", ResourceVersionId::new)]
            );
            assert!(input_relation_version_ids.is_empty());
        }
        other => panic!("expected inferred evidence, got {other:?}"),
    }
}

#[test]
fn inference_origin_defaults_relation_inputs_for_legacy_payloads() {
    let legacy = serde_json::json!({
        "type": "inference",
        "rule_version": "fixture-rule-v1",
        "input_resource_version_ids": ["fixture-resource-version"],
    });

    let origin: OriginRef = serde_json::from_value(legacy).unwrap();
    match origin {
        OriginRef::Inference {
            input_resource_version_ids,
            input_relation_version_ids,
            ..
        } => {
            assert_eq!(
                input_resource_version_ids,
                vec![id("fixture-resource-version", ResourceVersionId::new)]
            );
            assert!(input_relation_version_ids.is_empty());
        }
        other => panic!("expected inference origin, got {other:?}"),
    }
}

#[test]
fn binding_change_subject_round_trips_through_json() {
    let subject = ChangeSubject::Binding {
        binding_id: id("fixture-binding", BindingId::new),
    };

    let encoded = serde_json::to_value(&subject).unwrap();
    assert_eq!(
        encoded,
        serde_json::json!({
            "type": "binding",
            "binding_id": "fixture-binding",
        })
    );
    let decoded: ChangeSubject = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded, subject);
}

#[test]
fn disabled_binding_status_round_trips_through_json() {
    let encoded = serde_json::to_string(&BindingStatus::Disabled).unwrap();
    assert_eq!(encoded, r#""disabled""#);

    let decoded: BindingStatus = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, BindingStatus::Disabled);
}

#[test]
fn confidence_is_bounded_to_basis_points() {
    assert_eq!(Confidence::from_basis_points(0).unwrap().basis_points(), 0);
    assert_eq!(
        Confidence::from_basis_points(10_000)
            .unwrap()
            .basis_points(),
        10_000
    );
    assert!(Confidence::from_basis_points(10_001).is_err());
}

#[test]
fn secret_values_are_redacted_and_separate_from_serializable_references() {
    let secret_ref = SecretRef::new(SecretRefInput {
        backend: SecretBackend::MacosDataProtectionKeychainV1,
        service: "dev.example.next-infra.provider-secret.v1".into(),
        account: "connection/fixture-connection/kind/api-token/generation/fixture-generation"
            .into(),
        secret_kind: SecretKind::ApiToken,
        generation_id: "fixture-generation".into(),
        created_at: Timestamp::from_unix_millis(1).unwrap(),
        last_verified_at: Timestamp::from_unix_millis(2).unwrap(),
        permission_scope_summary: "fixture read-only scope".into(),
    })
    .unwrap();
    let secret_value = SecretValue::new(b"fixture-sensitive-value".to_vec());

    assert_eq!(
        serde_json::to_string(&secret_ref).unwrap(),
        r#"{"backend":"macos_data_protection_keychain_v1","service":"dev.example.next-infra.provider-secret.v1","account":"connection/fixture-connection/kind/api-token/generation/fixture-generation","secret_kind":"api_token","generation_id":"fixture-generation","created_at":1,"last_verified_at":2,"permission_scope_summary":"fixture read-only scope"}"#
    );
    assert_eq!(secret_value.expose(), b"fixture-sensitive-value");
    assert_eq!(format!("{secret_value:?}"), "SecretValue([REDACTED])");
    assert!(!format!("{secret_value:?}").contains("fixture-sensitive-value"));
    assert!(!format!("{secret_ref:?}").contains("dev.example.next-infra"));
    assert!(!format!("{secret_ref:?}").contains("fixture read-only scope"));
    assert_ne!(TypeId::of::<SecretRef>(), TypeId::of::<SecretValue>());
}

#[test]
fn resource_connector_freshness_and_lifecycle_are_independent_enums() {
    assert_ne!(
        TypeId::of::<ResourceHealth>(),
        TypeId::of::<ConnectorHealth>()
    );
    assert_ne!(TypeId::of::<ResourceHealth>(), TypeId::of::<Freshness>());
    assert_ne!(TypeId::of::<ResourceHealth>(), TypeId::of::<Lifecycle>());
    assert_ne!(TypeId::of::<ConnectorHealth>(), TypeId::of::<Freshness>());
    assert_ne!(TypeId::of::<ConnectorHealth>(), TypeId::of::<Lifecycle>());
    assert_ne!(TypeId::of::<Freshness>(), TypeId::of::<Lifecycle>());

    assert_eq!(
        serde_json::to_string(&ResourceHealth::Healthy).unwrap(),
        r#""healthy""#
    );
    assert_eq!(
        serde_json::to_string(&ConnectorHealth::Healthy).unwrap(),
        r#""healthy""#
    );
    assert_eq!(
        serde_json::to_string(&Freshness::Fresh).unwrap(),
        r#""fresh""#
    );
    assert_eq!(
        serde_json::to_string(&Lifecycle::Active).unwrap(),
        r#""active""#
    );
}
