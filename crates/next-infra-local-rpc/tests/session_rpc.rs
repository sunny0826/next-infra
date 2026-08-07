#![cfg(unix)]

use std::collections::BTreeSet;
use std::io;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Barrier, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use next_infra_local_rpc::protocol::{
    Caller, ClientHello, ErrorCode, GetResourceQuery, GetTopologyQuery, HandshakeResponse,
    HostHello, QueryRequest, QueryResponse, RecentChangesQuery, RequestEnvelope, ResourceInclude,
    ResponseBody, RpcError, SearchResourcesQuery, SyncStatusQuery, handshake_response,
};
use next_infra_local_rpc::session::{
    QueryHandler, QueryServiceHandler, RpcClient, RpcServer, SessionError,
};
use next_infra_local_rpc::transport::{
    FramedError, SecureUnixListener, TransportPaths, read_json_frame, write_json_frame,
};
use next_infra_query::dto::{
    ConnectionDto, ConnectorHealth, Freshness, Lifecycle, ResourceDto, ResourceHealth,
    SchemaVersion, SnapshotMetadata,
};
use next_infra_query::service::{
    HealthSummaryBody, QueryService, QuerySource, RecentChangesPlan, ResourceDetailBody,
    ResourceSearchPlan, SourcePage, SourceSnapshot, SyncStatusBody, TimelinePlan,
    TimelineSourcePage, TopologyBody, TopologyPlan,
};
use tempfile::tempdir;

#[test]
fn real_owner_only_unix_socket_connects_and_queries() {
    let root = tempdir().unwrap();
    let paths = TransportPaths::from_root(root.path()).unwrap();
    let listener = SecureUnixListener::bind(&paths).unwrap();
    let server = RpcServer::new(
        HostHello::initial("host-1.0.0", "release-a"),
        QueryServiceHandler::new(QueryService::new(FixtureSource)),
    );
    let server_thread = thread::spawn(move || server.serve_once(&listener));

    let hello = ClientHello::initial("bridge-1.0.0", "release-a");
    let mut client = RpcClient::connect(&paths, &hello).unwrap();
    let request = RequestEnvelope::new(
        "real-socket-query",
        Caller::test("session-test"),
        QueryRequest::GetHealthSummary,
    )
    .unwrap();
    let response = client.query(&request).unwrap();
    assert!(matches!(response.body, ResponseBody::Query(_)));

    drop(client);
    server_thread.join().unwrap().unwrap();
}

#[test]
fn handshake_and_all_seven_query_service_routes_round_trip() {
    let (client_stream, server_stream) = UnixStream::pair().unwrap();
    let server = RpcServer::new(
        HostHello::initial("host-1.0.0", "release-a"),
        QueryServiceHandler::new(QueryService::new(FixtureSource)),
    );
    let server_thread = thread::spawn(move || server.serve_stream(server_stream));

    let hello = ClientHello::initial("bridge-1.0.0", "release-a");
    let mut client = RpcClient::handshake(client_stream, &hello).unwrap();
    assert_eq!(client.host().selected_protocol_minor, 0);
    assert!(!client.upgrade_recommended());

    for (index, query) in all_queries().into_iter().enumerate() {
        let expected = query.capability();
        let request = RequestEnvelope::new(
            format!("request-{index}"),
            Caller::test("session-test"),
            query,
        )
        .unwrap();
        let response = client.query(&request).unwrap();
        match response.body {
            ResponseBody::Query(response) => assert_eq!(response.capability(), expected),
            ResponseBody::Error(error) => panic!("unexpected query error: {error}"),
        }
    }

    drop(client);
    server_thread.join().unwrap().unwrap();
}

#[test]
fn handshake_with_timeout_rejects_a_peer_that_withholds_response() {
    let (client_stream, server_stream) = UnixStream::pair().unwrap();
    let (release_tx, release_rx) = mpsc::channel();
    let server_thread = thread::spawn(move || {
        let _server_stream = server_stream;
        release_rx.recv().unwrap();
    });

    let hello = ClientHello::initial("bridge-1.0.0", "release-a");
    let started_at = Instant::now();
    let result =
        RpcClient::handshake_with_timeout(client_stream, &hello, Duration::from_millis(50));
    let elapsed = started_at.elapsed();

    release_tx.send(()).unwrap();
    server_thread.join().unwrap();

    let error = match result {
        Ok(_) => panic!("withholding peer must not complete handshake"),
        Err(error) => error,
    };
    assert!(
        elapsed < Duration::from_secs(1),
        "handshake took {elapsed:?}"
    );
    assert!(matches!(
        error,
        SessionError::Frame(FramedError::Io(error))
            if matches!(error.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock)
    ));
}

#[test]
fn query_with_timeout_rejects_a_peer_that_withholds_response() {
    let (client_stream, mut server_stream) = UnixStream::pair().unwrap();
    let server_thread = thread::spawn(move || {
        let client: ClientHello = read_json_frame(&mut server_stream).unwrap();
        let host = HostHello::initial("host-1.0.0", "release-a");
        write_json_frame(&mut server_stream, &handshake_response(&client, &host)).unwrap();
        let _: RequestEnvelope = read_json_frame(&mut server_stream).unwrap();
        thread::sleep(Duration::from_millis(200));
    });

    let hello = ClientHello::initial("bridge-1.0.0", "release-a");
    let mut client = RpcClient::handshake(client_stream, &hello).unwrap();
    let request = RequestEnvelope::new(
        "withheld-query",
        Caller::test("session-test"),
        QueryRequest::GetHealthSummary,
    )
    .unwrap();
    let started_at = Instant::now();
    let result = client.query_with_timeout(&request, Duration::from_millis(50));
    assert!(started_at.elapsed() < Duration::from_secs(1));
    assert!(matches!(
        result,
        Err(SessionError::Frame(FramedError::Io(error)))
            if error.kind() == io::ErrorKind::TimedOut
    ));
    server_thread.join().unwrap();
}

#[test]
fn request_before_handshake_receives_typed_rejection() {
    let (mut client, server_stream) = UnixStream::pair().unwrap();
    let server = RpcServer::new(
        HostHello::initial("host-1.0.0", "release-a"),
        QueryServiceHandler::new(QueryService::new(FixtureSource)),
    );
    let server_thread = thread::spawn(move || server.serve_stream(server_stream));

    let request = RequestEnvelope::new(
        "request-before-handshake",
        Caller::test("session-test"),
        QueryRequest::GetHealthSummary,
    )
    .unwrap();
    write_json_frame(&mut client, &request).unwrap();
    let response: HandshakeResponse = read_json_frame(&mut client).unwrap();
    assert!(matches!(
        response,
        HandshakeResponse::Rejected {
            error: RpcError {
                code: ErrorCode::InvalidFrame,
                ..
            }
        }
    ));
    assert!(server_thread.join().unwrap().is_err());
}

#[test]
fn incompatible_major_receives_protocol_mismatch() {
    let (mut client, server_stream) = UnixStream::pair().unwrap();
    let server = RpcServer::new(
        HostHello::initial("host-1.0.0", "release-a"),
        QueryServiceHandler::new(QueryService::new(FixtureSource)),
    );
    let server_thread = thread::spawn(move || server.serve_stream(server_stream));

    let mut hello = ClientHello::initial("bridge-1.0.0", "release-a");
    hello.protocol_major = 2;
    write_json_frame(&mut client, &hello).unwrap();
    let response: HandshakeResponse = read_json_frame(&mut client).unwrap();
    assert!(matches!(
        response,
        HandshakeResponse::Rejected {
            error: RpcError {
                code: ErrorCode::ProtocolMismatch,
                ..
            }
        }
    ));
    assert!(server_thread.join().unwrap().is_err());
}

#[test]
fn ninth_in_flight_request_is_rejected_without_queueing() {
    let barrier = Arc::new(Barrier::new(9));
    let handler = BlockingHealthHandler {
        barrier: Arc::clone(&barrier),
    };
    let (mut client, server_stream) = UnixStream::pair().unwrap();
    let server = RpcServer::new(HostHello::initial("host", "release"), handler);
    let server_thread = thread::spawn(move || server.serve_stream(server_stream));

    let hello = ClientHello::initial("bridge", "release");
    write_json_frame(&mut client, &hello).unwrap();
    let accepted: HandshakeResponse = read_json_frame(&mut client).unwrap();
    assert!(matches!(accepted, HandshakeResponse::Accepted { .. }));

    for index in 0..9 {
        let request = RequestEnvelope::new(
            format!("request-{index}"),
            Caller::test("concurrency-test"),
            QueryRequest::GetHealthSummary,
        )
        .unwrap();
        write_json_frame(&mut client, &request).unwrap();
    }
    client.shutdown(Shutdown::Write).unwrap();

    let rejected: next_infra_local_rpc::protocol::ResponseEnvelope =
        read_json_frame(&mut client).unwrap();
    assert_eq!(rejected.request_id, "request-8");
    assert!(matches!(
        rejected.body,
        ResponseBody::Error(RpcError {
            code: ErrorCode::TooManyRequests,
            ..
        })
    ));

    barrier.wait();
    let mut successes = BTreeSet::new();
    for _ in 0..8 {
        let response: next_infra_local_rpc::protocol::ResponseEnvelope =
            read_json_frame(&mut client).unwrap();
        assert!(matches!(response.body, ResponseBody::Query(_)));
        successes.insert(response.request_id);
    }
    assert_eq!(successes.len(), 8);
    server_thread.join().unwrap().unwrap();
}

struct BlockingHealthHandler {
    barrier: Arc<Barrier>,
}

impl QueryHandler for BlockingHealthHandler {
    fn handle(&self, query: QueryRequest) -> Result<QueryResponse, RpcError> {
        assert!(matches!(query, QueryRequest::GetHealthSummary));
        self.barrier.wait();
        QueryServiceHandler::new(QueryService::new(FixtureSource)).handle(query)
    }
}

#[derive(Clone, Copy)]
struct FixtureSource;

impl QuerySource for FixtureSource {
    type Error = ();

    fn search_resources(
        &self,
        _plan: &ResourceSearchPlan,
    ) -> Result<SourceSnapshot<SourcePage<ResourceDto>>, Self::Error> {
        Ok(snapshot(SourcePage {
            items: vec![resource()],
            next_after: None,
        }))
    }

    fn get_resource(
        &self,
        _resource_id: &str,
        _include: &BTreeSet<next_infra_query::service::ResourceInclude>,
    ) -> Result<SourceSnapshot<Option<ResourceDetailBody>>, Self::Error> {
        Ok(snapshot(Some(ResourceDetailBody {
            resource: resource(),
            attributes: serde_json::json!({"fixture": true}),
            relations: vec![],
            recent_changes: vec![],
            connector_coverage: vec![],
        })))
    }

    fn get_topology(
        &self,
        _plan: &TopologyPlan,
    ) -> Result<SourceSnapshot<Option<TopologyBody>>, Self::Error> {
        Ok(snapshot(Some(TopologyBody {
            nodes: vec![resource()],
            edges: vec![],
            frontier: vec![],
            truncated: false,
        })))
    }

    fn get_health_summary(&self) -> Result<SourceSnapshot<HealthSummaryBody>, Self::Error> {
        Ok(snapshot(HealthSummaryBody {
            resource_health: Default::default(),
            freshness: Default::default(),
            connector_health: Default::default(),
        }))
    }

    fn list_connections(&self) -> Result<SourceSnapshot<Vec<ConnectionDto>>, Self::Error> {
        Ok(snapshot(vec![]))
    }

    fn get_recent_changes(
        &self,
        _plan: &RecentChangesPlan,
    ) -> Result<SourceSnapshot<SourcePage<next_infra_query::dto::ChangeDto>>, Self::Error> {
        Ok(snapshot(SourcePage {
            items: vec![],
            next_after: None,
        }))
    }

    fn get_sync_status(
        &self,
        _connection_id: &str,
        _recent_run_limit: usize,
    ) -> Result<SourceSnapshot<Option<SyncStatusBody>>, Self::Error> {
        Ok(snapshot(Some(SyncStatusBody {
            connection: ConnectionDto {
                connection_id: "connection-1".into(),
                connector_type: "fixture".into(),
                display_name: "Fixture".into(),
                enabled: true,
                health: ConnectorHealth::Healthy,
                last_success_at: None,
                last_attempt_at: None,
            },
            recent_runs: vec![],
            next_scheduled_at: None,
        })))
    }

    fn get_timeline(
        &self,
        _plan: &TimelinePlan,
    ) -> Result<SourceSnapshot<TimelineSourcePage>, Self::Error> {
        Ok(snapshot(TimelineSourcePage {
            groups: vec![],
            item_count: 0,
            next_after: None,
        }))
    }

    fn list_connector_coverage(
        &self,
    ) -> Result<SourceSnapshot<Vec<next_infra_query::dto::ConnectorCoverageDto>>, Self::Error> {
        Ok(snapshot(vec![]))
    }
}

fn all_queries() -> Vec<QueryRequest> {
    vec![
        QueryRequest::SearchResources(SearchResourcesQuery::default()),
        QueryRequest::GetResource(GetResourceQuery {
            resource_id: "resource-1".into(),
            include: [ResourceInclude::Attributes].into_iter().collect(),
        }),
        QueryRequest::GetTopology(GetTopologyQuery {
            focus_resource_id: "resource-1".into(),
            depth: Some(1),
            max_nodes: Some(10),
            max_edges: Some(20),
        }),
        QueryRequest::GetHealthSummary,
        QueryRequest::GetRecentChanges(RecentChangesQuery::default()),
        QueryRequest::GetSyncStatus(SyncStatusQuery {
            connection_id: "connection-1".into(),
            recent_run_limit: Some(10),
        }),
        QueryRequest::ListConnectorCoverage,
    ]
}

fn snapshot<T>(body: T) -> SourceSnapshot<T> {
    SourceSnapshot {
        metadata: SnapshotMetadata {
            schema_version: SchemaVersion::new(1),
            snapshot_version: "snapshot-1".into(),
            generated_at: "2026-08-04T00:00:00Z".into(),
        },
        body,
    }
}

fn resource() -> ResourceDto {
    ResourceDto {
        resource_id: "resource-1".into(),
        connection_id: "connection-1".into(),
        kind: "fixture".into(),
        display_name: "Fixture resource".into(),
        scope: "default".into(),
        lifecycle: Lifecycle::Active,
        health: ResourceHealth::Healthy,
        freshness: Freshness::Fresh,
        observed_at: "2026-08-04T00:00:00Z".into(),
    }
}
