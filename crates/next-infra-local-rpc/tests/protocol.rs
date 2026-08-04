use next_infra_local_rpc::protocol::*;
use next_infra_query::dto::{
    ChangePageDto, ConnectionDto, ConnectorCoverageSnapshotDto, ConnectorHealth,
    ConnectorHealthCountsDto, Freshness, FreshnessCountsDto, HealthSummaryDto, Lifecycle, PageInfo,
    ResourceDetailDto, ResourceDto, ResourceHealth, ResourceHealthCountsDto, ResourcePageDto,
    SchemaVersion, SnapshotMetadata, SyncStatusDto, TopologyDto,
};
use serde_json::{Value, json};
use std::collections::BTreeSet;

const FIXTURE_CLIENT: &str = include_str!("fixtures/protocol/client_hello_1_0.json");
const FIXTURE_HOST: &str = include_str!("fixtures/protocol/host_hello_1_0.json");

fn all_capabilities() -> CapabilitySet {
    CapabilitySet::all_query_capabilities()
}

fn metadata() -> SnapshotMetadata {
    SnapshotMetadata {
        schema_version: SchemaVersion::new(1),
        snapshot_version: "snapshot-1".into(),
        generated_at: "2026-08-04T00:00:00Z".into(),
    }
}

fn resource() -> ResourceDto {
    ResourceDto {
        resource_id: "resource-1".into(),
        connection_id: "connection-1".into(),
        kind: "database".into(),
        display_name: "Fixture resource".into(),
        scope: "default".into(),
        lifecycle: Lifecycle::Active,
        health: ResourceHealth::Healthy,
        freshness: Freshness::Fresh,
        observed_at: "2026-08-04T00:00:00Z".into(),
    }
}

fn response_variants() -> Vec<QueryResponse> {
    let resource = resource();
    vec![
        QueryResponse::SearchResources(ResourcePageDto {
            metadata: metadata(),
            items: vec![resource.clone()],
            page_info: PageInfo::new(None),
        }),
        QueryResponse::GetResource(ResourceDetailDto {
            metadata: metadata(),
            resource: resource.clone(),
            attributes: json!({"engine":"fixture"}),
            relations: vec![],
            recent_changes: vec![],
            connector_coverage: vec![],
        }),
        QueryResponse::GetTopology(TopologyDto {
            metadata: metadata(),
            focus_resource_id: resource.resource_id.clone(),
            depth: 1,
            nodes: vec![resource],
            edges: vec![],
            frontier: vec![],
            truncated: false,
        }),
        QueryResponse::GetHealthSummary(HealthSummaryDto {
            metadata: metadata(),
            resource_health: ResourceHealthCountsDto::default(),
            freshness: FreshnessCountsDto::default(),
            connector_health: ConnectorHealthCountsDto::default(),
        }),
        QueryResponse::GetRecentChanges(ChangePageDto {
            metadata: metadata(),
            items: vec![],
            page_info: PageInfo::new(None),
        }),
        QueryResponse::GetSyncStatus(SyncStatusDto {
            metadata: metadata(),
            connection: ConnectionDto {
                connection_id: "connection-1".into(),
                connector_type: "fixture".into(),
                display_name: "Fixture connection".into(),
                enabled: true,
                health: ConnectorHealth::Healthy,
                last_success_at: None,
                last_attempt_at: None,
            },
            recent_runs: vec![],
            next_scheduled_at: None,
        }),
        QueryResponse::ListConnectorCoverage(ConnectorCoverageSnapshotDto {
            metadata: metadata(),
            items: vec![],
        }),
    ]
}

#[test]
fn hello_golden_fixtures_are_canonical_and_round_trip() {
    let client: ClientHello = serde_json::from_str(FIXTURE_CLIENT).unwrap();
    let host: HostHello = serde_json::from_str(FIXTURE_HOST).unwrap();
    assert_eq!(
        serde_json::to_string(&client).unwrap(),
        FIXTURE_CLIENT.trim_end()
    );
    assert_eq!(
        serde_json::to_string(&host).unwrap(),
        FIXTURE_HOST.trim_end()
    );
    assert_eq!(
        negotiate(&client, &host).unwrap().selected_protocol_minor,
        0
    );
}

#[test]
fn handshake_covers_same_and_adjacent_minor_windows() {
    let cases = [
        (2, 1, 2, 1, 2), // N/N
        (2, 1, 1, 0, 1), // N/N-1
        (1, 0, 2, 1, 1), // N-1/N
    ];
    for (client_minor, client_min, host_minor, host_min, selected) in cases {
        let client = ClientHello::new(
            client_minor,
            client_min,
            "bridge",
            "release",
            CapabilitySet::empty(),
            all_capabilities(),
        );
        let host = HostHello::new(
            host_minor,
            host_min,
            selected,
            "host",
            "release",
            all_capabilities(),
            CapabilitySet::empty(),
        );
        let result = negotiate(&client, &host).unwrap();
        assert_eq!(result.selected_protocol_minor, selected);
        assert!(!result.upgrade_recommended);
    }
}

#[test]
fn handshake_rejects_no_overlap_and_major_mismatch() {
    let client = ClientHello::new(
        2,
        2,
        "bridge",
        "release",
        CapabilitySet::empty(),
        all_capabilities(),
    );
    let host = HostHello::new(
        1,
        0,
        0,
        "host",
        "release",
        all_capabilities(),
        CapabilitySet::empty(),
    );
    assert_eq!(
        negotiate(&client, &host).unwrap_err().code,
        ErrorCode::ProtocolMismatch
    );

    let mut major_mismatch = HostHello::initial("host", "release");
    major_mismatch.protocol_major = 2;
    assert_eq!(
        negotiate(&ClientHello::initial("bridge", "release"), &major_mismatch)
            .unwrap_err()
            .code,
        ErrorCode::ProtocolMismatch
    );
}

#[test]
fn handshake_checks_capabilities_in_both_directions_and_release_upgrade() {
    let client = ClientHello::new(
        0,
        0,
        "bridge",
        "release-a",
        CapabilitySet::empty(),
        all_capabilities(),
    );
    let host_without_query = HostHello::new(
        0,
        0,
        0,
        "host",
        "release-a",
        CapabilitySet::empty(),
        CapabilitySet::empty(),
    );
    assert_eq!(
        negotiate(&client, &host_without_query).unwrap_err().code,
        ErrorCode::CapabilityMismatch
    );

    let client_without_required = ClientHello::new(
        0,
        0,
        "bridge",
        "release-a",
        CapabilitySet::empty(),
        CapabilitySet::empty(),
    );
    let host_requires_query = HostHello::new(
        0,
        0,
        0,
        "host",
        "release-a",
        CapabilitySet::empty(),
        all_capabilities(),
    );
    assert_eq!(
        negotiate(&client_without_required, &host_requires_query)
            .unwrap_err()
            .code,
        ErrorCode::CapabilityMismatch
    );

    let host = HostHello::new(
        0,
        0,
        0,
        "host",
        "release-b",
        all_capabilities(),
        CapabilitySet::empty(),
    );
    assert!(
        negotiate(&client_without_required, &host)
            .unwrap()
            .upgrade_recommended
    );
}

#[test]
fn capability_sets_are_sorted_and_reject_duplicates_or_unknown_values() {
    let json = r#"["query.search_resources.v1","query.get_resource.v1"]"#;
    let set: CapabilitySet = serde_json::from_str(json).unwrap();
    assert_eq!(
        serde_json::to_string(&set).unwrap(),
        r#"["query.get_resource.v1","query.search_resources.v1"]"#
    );
    assert!(
        serde_json::from_str::<CapabilitySet>(
            r#"["query.get_resource.v1","query.get_resource.v1"]"#
        )
        .is_err()
    );
    assert!(serde_json::from_str::<CapabilitySet>(r#"["query.execute.v1"]"#).is_err());
}

#[test]
fn all_query_requests_and_responses_round_trip() {
    let queries = vec![
        QueryRequest::SearchResources(Default::default()),
        QueryRequest::GetResource(GetResourceQuery {
            resource_id: "resource-1".into(),
            include: BTreeSet::new(),
        }),
        QueryRequest::GetTopology(GetTopologyQuery {
            focus_resource_id: "resource-1".into(),
            depth: Some(1),
            max_nodes: Some(10),
            max_edges: Some(20),
        }),
        QueryRequest::GetHealthSummary,
        QueryRequest::GetRecentChanges(Default::default()),
        QueryRequest::GetSyncStatus(SyncStatusQuery {
            connection_id: "connection-1".into(),
            recent_run_limit: Some(10),
        }),
        QueryRequest::ListConnectorCoverage,
    ];
    for query in queries {
        let request =
            RequestEnvelope::new("request-1", Caller::bridge("1.0", "release"), query).unwrap();
        let frame = encode_frame(&request).unwrap();
        let decoded: RequestEnvelope = decode_frame(&frame).unwrap();
        assert_eq!(decoded, request);
    }

    for response in response_variants() {
        let envelope = ResponseEnvelope::success("request-1", response).unwrap();
        let frame = encode_frame(&envelope).unwrap();
        let decoded: ResponseEnvelope = decode_frame(&frame).unwrap();
        assert_eq!(decoded, envelope);
    }
}

#[test]
fn response_errors_round_trip_with_stable_codes() {
    let errors = [
        RpcError::host_unavailable("Host is not running."),
        RpcError::protocol_mismatch("Protocol versions do not overlap."),
        RpcError::capability_mismatch("Required capability is unavailable."),
        RpcError::oversized_frame("Frame is too large."),
        RpcError::invalid_frame("Frame is malformed."),
        RpcError::invalid_request_id(),
        RpcError::too_many_requests(),
        RpcError::query_failed("Query failed.", true),
    ];
    for error in errors {
        let envelope = ResponseEnvelope::error("request-1", error).unwrap();
        let frame = encode_frame(&envelope).unwrap();
        let decoded: ResponseEnvelope = decode_frame(&frame).unwrap();
        assert_eq!(decoded, envelope);
    }
    assert_eq!(
        serde_json::to_string(&ErrorCode::FrameTooLarge).unwrap(),
        "\"frame_too_large\""
    );
    assert!(serde_json::from_str::<ErrorCode>("\"oversized_frame\"").is_err());
}

#[test]
fn frame_prefix_is_exact_big_endian_length() {
    let frame = encode_frame(&json!({"a": 1})).unwrap();
    assert_eq!(&frame[..4], &[0, 0, 0, 7]);
    assert_eq!(&frame[4..], br#"{"a":1}"#);
    assert_eq!(decode_frame_bytes(&frame).unwrap(), br#"{"a":1}"#);
}

#[test]
fn frame_boundary_and_error_cases_are_strict() {
    let body = Value::String("x".repeat(MAX_FRAME_BYTES - 2));
    let frame = encode_frame(&body).unwrap();
    assert_eq!(
        u32::from_be_bytes(frame[..4].try_into().unwrap()) as usize,
        MAX_FRAME_BYTES
    );
    let decoded: Value = decode_frame(&frame).unwrap();
    assert_eq!(decoded, body);

    let too_large = Value::String("x".repeat(MAX_FRAME_BYTES - 1));
    assert!(matches!(
        encode_frame(&too_large),
        Err(FrameError::OversizedFrame { .. })
    ));

    let mut oversized_header = vec![0, 0x10, 0, 1]; // 1 MiB + 1 byte
    assert!(matches!(
        decode_frame_bytes(&oversized_header),
        Err(FrameError::OversizedFrame { .. })
    ));
    oversized_header[0] = 0;

    assert!(matches!(
        decode_frame_bytes(&[0, 0, 0]),
        Err(FrameError::InvalidFrame(
            FrameErrorKind::MissingHeader { .. }
        ))
    ));
    assert!(matches!(
        decode_frame_bytes(&[0, 0, 0, 0]),
        Err(FrameError::InvalidFrame(FrameErrorKind::ZeroLength))
    ));
    assert!(matches!(
        decode_frame_bytes(&[0, 0, 0, 2, b'{']),
        Err(FrameError::InvalidFrame(FrameErrorKind::Incomplete { .. }))
    ));

    let mut trailing = encode_frame(&json!(null)).unwrap();
    trailing.push(b' ');
    assert!(matches!(
        decode_frame_bytes(&trailing),
        Err(FrameError::InvalidFrame(
            FrameErrorKind::TrailingBytes { .. }
        ))
    ));

    let invalid_utf8 = [0, 0, 0, 1, 0xff];
    assert!(matches!(
        decode_frame::<Value>(&invalid_utf8),
        Err(FrameError::InvalidFrame(FrameErrorKind::InvalidJson(_)))
    ));
}

#[test]
fn request_ids_use_utf8_bytes_and_in_flight_limit() {
    let caller = Caller::test("protocol-test");
    assert!(
        RequestEnvelope::new(
            "a".repeat(MAX_REQUEST_ID_BYTES),
            caller.clone(),
            QueryRequest::GetHealthSummary,
        )
        .unwrap()
        .request_id_is_at_limit()
    );
    assert!(
        RequestEnvelope::new(
            "a".repeat(MAX_REQUEST_ID_BYTES + 1),
            caller.clone(),
            QueryRequest::GetHealthSummary,
        )
        .is_err()
    );
    assert!(
        RequestEnvelope::new(
            "é".repeat(MAX_REQUEST_ID_BYTES / 2),
            caller.clone(),
            QueryRequest::GetHealthSummary,
        )
        .is_ok()
    );
    assert!(
        RequestEnvelope::new(
            "é".repeat(MAX_REQUEST_ID_BYTES / 2 + 1),
            caller,
            QueryRequest::GetHealthSummary,
        )
        .is_err()
    );

    assert!(validate_in_flight(MAX_IN_FLIGHT_REQUESTS - 1).is_ok());
    assert_eq!(
        validate_in_flight(MAX_IN_FLIGHT_REQUESTS).unwrap_err().code,
        ErrorCode::TooManyRequests
    );
}

#[test]
fn arbitrary_method_and_params_do_not_deserialize_as_a_query() {
    let value = json!({
        "request_id": "request-1",
        "caller": {"type": "test", "name": "fixture"},
        "query": {"method": "execute_sql", "params": {"sql": "select 1"}}
    });
    assert!(serde_json::from_value::<RequestEnvelope>(value).is_err());
}

#[test]
fn wire_deserialization_validates_request_ids_and_callers() {
    let base = json!({
        "request_id": "request-1",
        "caller": {"type": "test", "name": "fixture"},
        "query": {"type": "get_health_summary"}
    });
    let mut overlong = base.clone();
    overlong["request_id"] = Value::String("a".repeat(MAX_REQUEST_ID_BYTES + 1));
    assert!(serde_json::from_value::<RequestEnvelope>(overlong.clone()).is_err());
    assert!(decode_frame::<RequestEnvelope>(&encode_frame(&overlong).unwrap()).is_err());

    let mut control_id = base.clone();
    control_id["request_id"] = Value::String("request\n1".into());
    assert!(serde_json::from_value::<RequestEnvelope>(control_id).is_err());

    let mut control_caller = base;
    control_caller["caller"]["name"] = Value::String("fixture\n".into());
    assert!(serde_json::from_value::<RequestEnvelope>(control_caller).is_err());

    let response = json!({
        "request_id": "",
        "body": {
            "type": "error",
            "data": {"code": "invalid_frame", "message": "bad", "retryable": false}
        }
    });
    assert!(serde_json::from_value::<ResponseEnvelope>(response).is_err());
}
