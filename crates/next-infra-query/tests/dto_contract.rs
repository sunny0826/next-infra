use next_infra_query::dto::{
    ChangeDto, ChangeOriginDto, ChangeSubjectDto, ConnectionDto, ConnectorCoverageDto,
    ConnectorCoverageLevelDto, ConnectorHealth, ErrorEnvelope, EvidenceType, FieldChangeDto,
    Freshness, Lifecycle, PageInfo, QUERY_DTO_SCHEMA_VERSION, QueryViewState, RelationDto,
    ResourceDto, ResourceHealth, SchemaVersion, SnapshotMetadata, SyncCoverageDto, SyncModeDto,
    SyncRunCountsDto, SyncRunDto, SyncRunStatusDto, SyncTriggerDto,
};
use serde::Serialize;
use serde_json::{Value, json};

fn field_names<T: Serialize>(value: &T) -> Vec<String> {
    let Value::Object(object) = serde_json::to_value(value).expect("DTO must serialize") else {
        panic!("DTO must serialize as an object");
    };

    let mut fields: Vec<_> = object.keys().cloned().collect();
    fields.sort();
    fields
}

fn assert_clean<T: Serialize>(value: &T) {
    let serialized = serde_json::to_string(value)
        .expect("DTO must serialize")
        .to_ascii_lowercase();

    for forbidden in ["secret", "credential", "token", "password"] {
        assert!(
            !serialized.contains(forbidden),
            "serialized DTO contains forbidden term: {forbidden}"
        );
    }
}

#[test]
fn schema_version_is_one() {
    assert_eq!(QUERY_DTO_SCHEMA_VERSION, SchemaVersion::new(1));
    assert_eq!(QUERY_DTO_SCHEMA_VERSION.get(), 1);
    assert_eq!(
        serde_json::to_value(QUERY_DTO_SCHEMA_VERSION).expect("schema version must serialize"),
        json!(1)
    );
}

#[test]
fn page_cursor_round_trips_as_an_opaque_value() {
    let page = PageInfo::new(Some("server-issued-value".to_owned()));

    assert_eq!(page.next_cursor(), Some("server-issued-value"));
    assert_eq!(
        serde_json::to_value(&page).expect("page info must serialize"),
        json!({ "next_cursor": "server-issued-value" })
    );
}

#[test]
fn status_enums_reject_unknown_values() {
    for invalid in ["deleted", "ACTIVE", ""] {
        assert!(serde_json::from_value::<Lifecycle>(json!(invalid)).is_err());
    }
    for invalid in ["ok", "down", ""] {
        assert!(serde_json::from_value::<ResourceHealth>(json!(invalid)).is_err());
    }
    for invalid in ["current", "old", ""] {
        assert!(serde_json::from_value::<Freshness>(json!(invalid)).is_err());
    }
    for invalid in ["binding", "guessed", ""] {
        assert!(serde_json::from_value::<EvidenceType>(json!(invalid)).is_err());
    }
    for invalid in ["offline", "failed", ""] {
        assert!(serde_json::from_value::<ConnectorHealth>(json!(invalid)).is_err());
    }
}

#[test]
fn status_enums_use_glossary_values() {
    assert_eq!(
        [
            Lifecycle::Active,
            Lifecycle::Tombstoned,
            Lifecycle::Orphaned,
        ]
        .map(|value| serde_json::to_value(value).expect("lifecycle must serialize")),
        [json!("active"), json!("tombstoned"), json!("orphaned")]
    );
    assert_eq!(
        [
            ResourceHealth::Healthy,
            ResourceHealth::Degraded,
            ResourceHealth::Unhealthy,
            ResourceHealth::Unknown,
        ]
        .map(|value| serde_json::to_value(value).expect("resource health must serialize")),
        [
            json!("healthy"),
            json!("degraded"),
            json!("unhealthy"),
            json!("unknown"),
        ]
    );
    assert_eq!(
        [Freshness::Fresh, Freshness::Stale, Freshness::Expired]
            .map(|value| serde_json::to_value(value).expect("freshness must serialize")),
        [json!("fresh"), json!("stale"), json!("expired")]
    );
    assert_eq!(
        [
            EvidenceType::Provider,
            EvidenceType::Configured,
            EvidenceType::Inferred,
        ]
        .map(|value| serde_json::to_value(value).expect("evidence type must serialize")),
        [json!("provider"), json!("configured"), json!("inferred")]
    );
    assert_eq!(
        [
            ConnectorHealth::Healthy,
            ConnectorHealth::Degraded,
            ConnectorHealth::AuthFailed,
            ConnectorHealth::RateLimited,
            ConnectorHealth::Unreachable,
            ConnectorHealth::Disabled,
        ]
        .map(|value| serde_json::to_value(value).expect("connector health must serialize")),
        [
            json!("healthy"),
            json!("degraded"),
            json!("auth_failed"),
            json!("rate_limited"),
            json!("unreachable"),
            json!("disabled"),
        ]
    );
}

#[test]
fn dto_shapes_are_stable_and_clean() {
    let snapshot = SnapshotMetadata {
        schema_version: QUERY_DTO_SCHEMA_VERSION,
        snapshot_version: "snapshot-1".to_owned(),
        generated_at: "2026-08-02T00:00:00Z".to_owned(),
    };
    let error = ErrorEnvelope {
        schema_version: QUERY_DTO_SCHEMA_VERSION,
        code: "not_found".to_owned(),
        message: "Resource not found".to_owned(),
        retryable: false,
    };
    let resource = ResourceDto {
        resource_id: "resource-1".to_owned(),
        connection_id: "connection-1".to_owned(),
        kind: "github.repository".to_owned(),
        display_name: "Example repository".to_owned(),
        scope: "example-scope".to_owned(),
        lifecycle: Lifecycle::Active,
        health: ResourceHealth::Healthy,
        freshness: Freshness::Fresh,
        observed_at: "2026-08-02T00:00:00Z".to_owned(),
    };
    let relation = RelationDto {
        relation_id: "relation-1".to_owned(),
        source_resource_id: "resource-1".to_owned(),
        target_resource_id: "resource-2".to_owned(),
        kind: "defines".to_owned(),
        lifecycle: Lifecycle::Active,
        evidence_type: EvidenceType::Provider,
        evidence: next_infra_query::dto::RelationEvidenceDto::Provider {
            connector_type: "github".to_owned(),
            connection_id: "connection-1".to_owned(),
            sync_run_id: "sync-run-1".to_owned(),
            field_path: "attributes.target".to_owned(),
        },
        last_seen_at: "2026-08-02T00:00:00Z".to_owned(),
    };
    let connection = ConnectionDto {
        connection_id: "connection-1".to_owned(),
        connector_type: "github".to_owned(),
        display_name: "Example account".to_owned(),
        enabled: true,
        health: ConnectorHealth::Healthy,
        last_success_at: Some("2026-08-02T00:00:00Z".to_owned()),
        last_attempt_at: Some("2026-08-02T00:00:00Z".to_owned()),
    };

    assert_eq!(
        field_names(&snapshot),
        ["generated_at", "schema_version", "snapshot_version"]
    );
    assert_eq!(
        field_names(&error),
        ["code", "message", "retryable", "schema_version"]
    );
    assert_eq!(
        field_names(&resource),
        [
            "connection_id",
            "display_name",
            "freshness",
            "health",
            "kind",
            "lifecycle",
            "observed_at",
            "resource_id",
            "scope",
        ]
    );
    assert_eq!(
        field_names(&relation),
        [
            "evidence",
            "evidence_type",
            "kind",
            "last_seen_at",
            "lifecycle",
            "relation_id",
            "source_resource_id",
            "target_resource_id",
        ]
    );
    assert_eq!(
        field_names(&connection),
        [
            "connection_id",
            "connector_type",
            "display_name",
            "enabled",
            "health",
            "last_attempt_at",
            "last_success_at",
        ]
    );

    assert_clean(&snapshot);
    assert_clean(&PageInfo::new(Some("next-page".to_owned())));
    assert_clean(&error);
    assert_clean(&resource);
    assert_clean(&relation);
    assert_clean(&connection);
}

#[test]
fn goal_two_history_and_coverage_dtos_are_tagged_and_clean() {
    let sync_run = SyncRunDto {
        sync_run_id: "fixture-run".into(),
        connection_id: "fixture-connection".into(),
        mode: SyncModeDto::Full,
        trigger: SyncTriggerDto::Schedule,
        status: SyncRunStatusDto::Partial,
        coverage: SyncCoverageDto::Partial {
            scope: Some("fixture-scope".into()),
            reason: "rate_limited".into(),
        },
        started_at: "2000-01-01T00:00:00Z".into(),
        finished_at: Some("2000-01-01T00:00:01Z".into()),
        cursor_before: Some("fixture-cursor-before".into()),
        cursor_after: Some("fixture-cursor-after".into()),
        counts: SyncRunCountsDto {
            read: 1,
            warnings: 1,
            ..SyncRunCountsDto::default()
        },
        errors: Vec::new(),
    };
    let coverage = ConnectorCoverageDto {
        connector_type: "fixture".into(),
        connector_version: "1.0.0".into(),
        module: "fixture.compute".into(),
        level: ConnectorCoverageLevelDto::Partial,
        reason: Some("fixture limitation".into()),
    };
    let change = ChangeDto {
        change_id: "fixture-change".into(),
        subject: ChangeSubjectDto::Resource {
            resource_id: "fixture-resource".into(),
        },
        observed_at: "2000-01-01T00:00:01Z".into(),
        fields: vec![FieldChangeDto {
            path: "attributes.state".into(),
            before: Some(json!("pending")),
            after: Some(json!("ready")),
        }],
        origin: ChangeOriginDto::SyncRun {
            sync_run_id: "fixture-run".into(),
        },
    };

    assert_eq!(
        serde_json::to_value(QueryViewState::Loading).unwrap(),
        json!("loading")
    );
    assert_eq!(
        serde_json::to_value(&sync_run.coverage).unwrap(),
        json!({
            "type": "partial",
            "scope": "fixture-scope",
            "reason": "rate_limited",
        })
    );
    assert_eq!(
        field_names(&sync_run),
        [
            "connection_id",
            "counts",
            "coverage",
            "cursor_after",
            "cursor_before",
            "errors",
            "finished_at",
            "mode",
            "started_at",
            "status",
            "sync_run_id",
            "trigger",
        ]
    );
    assert_clean(&sync_run);
    assert_clean(&coverage);
    assert_clean(&change);
}
