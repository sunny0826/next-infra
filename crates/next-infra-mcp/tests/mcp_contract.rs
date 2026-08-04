use std::collections::BTreeSet;
use std::sync::Mutex;

use next_infra_local_rpc::protocol::{
    Capability, GetResourceQuery, GetTopologyQuery, QueryRequest, QueryResponse,
    RecentChangesQuery, SearchResourcesQuery, SyncStatusQuery,
};
use next_infra_mcp::input::{
    GetResourceInput, GetTopologyInput, RecentChangesInput, SearchResourcesInput, SyncStatusInput,
};
use next_infra_mcp::{
    CAPABILITIES_RESOURCE_URI, HEALTH_RESOURCE_URI, McpBridgeError, McpQueryClient, NextInfraMcp,
    TOOL_NAMES,
};
use rmcp::handler::server::wrapper::Parameters;
use serde_json::json;

#[tokio::test]
async fn tools_are_exactly_seven_read_only_structured_routes() {
    let server = NextInfraMcp::new(FakeClient::default());
    let tools = server.tools();
    assert_eq!(tools.len(), 7);
    assert_eq!(
        tools
            .iter()
            .map(|tool| tool.name.as_ref())
            .collect::<BTreeSet<_>>(),
        TOOL_NAMES.into_iter().collect()
    );

    for tool in &tools {
        let annotations = tool.annotations.as_ref().expect("annotations");
        assert_eq!(annotations.read_only_hint, Some(true));
        assert_eq!(annotations.destructive_hint, Some(false));
        assert_eq!(annotations.idempotent_hint, Some(true));
        assert_eq!(annotations.open_world_hint, Some(false));
        assert!(!tool.input_schema.is_empty());
        assert!(tool.output_schema.is_some());
    }

    server
        .search_resources(Parameters(SearchResourcesInput::default()))
        .await
        .unwrap();
    server
        .get_resource(Parameters(GetResourceInput {
            resource_id: "resource-1".into(),
            include: vec![],
        }))
        .await
        .unwrap();
    let topology = server
        .get_topology(Parameters(GetTopologyInput {
            focus_resource_id: "resource-1".into(),
            depth: Some(1),
            max_nodes: Some(10),
            max_edges: Some(20),
        }))
        .await
        .unwrap();
    server.get_health_summary().await.unwrap();
    server
        .get_recent_changes(Parameters(RecentChangesInput::default()))
        .await
        .unwrap();
    server
        .get_sync_status(Parameters(SyncStatusInput {
            connection_id: "connection-1".into(),
            recent_run_limit: Some(10),
        }))
        .await
        .unwrap();
    server.list_connector_coverage().await.unwrap();

    assert_eq!(topology.0.observed_at, "2026-08-04T00:00:00Z");
    assert_eq!(topology.0.data["data"]["truncated"], true);

    let seen = server
        .read_resource_json(HEALTH_RESOURCE_URI)
        .await
        .unwrap();
    assert!(seen.contains("observed_at"));
    let capabilities = server
        .read_resource_json(CAPABILITIES_RESOURCE_URI)
        .await
        .unwrap();
    assert!(capabilities.contains("search_resources"));
    assert_eq!(server.resources().len(), 2);
    assert!(
        server
            .read_resource_json("next-infra://missing")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn rpc_failure_is_safe_tool_error_text() {
    let server = NextInfraMcp::new(FakeClient {
        seen: Mutex::new(vec![]),
        fail: true,
    });
    let error = match server.get_health_summary().await {
        Ok(_) => panic!("expected tool error"),
        Err(error) => error,
    };
    assert!(error.contains("host_unavailable"));
    assert!(!error.contains("/Users/"));
    assert!(!error.to_ascii_lowercase().contains("select "));
}

#[derive(Default)]
struct FakeClient {
    seen: Mutex<Vec<Capability>>,
    fail: bool,
}

impl McpQueryClient for FakeClient {
    fn query(&self, query: QueryRequest) -> Result<QueryResponse, McpBridgeError> {
        self.seen.lock().unwrap().push(query.capability());
        if self.fail {
            return Err(McpBridgeError::new(
                "host_unavailable",
                "The local Host is unavailable.",
                true,
            ));
        }
        Ok(response_for(query))
    }
}

fn response_for(query: QueryRequest) -> QueryResponse {
    let value = match query {
        QueryRequest::SearchResources(SearchResourcesQuery { .. }) => json!({
            "type": "search_resources",
            "data": {"metadata": metadata_json(), "items": [resource_json()], "page_info": {"next_cursor": "niq1:next"}}
        }),
        QueryRequest::GetResource(GetResourceQuery { .. }) => json!({
            "type": "get_resource",
            "data": {"metadata": metadata_json(), "resource": resource_json(), "attributes": {"fixture": true}, "relations": [], "recent_changes": [], "connector_coverage": []}
        }),
        QueryRequest::GetTopology(GetTopologyQuery { depth, .. }) => json!({
            "type": "get_topology",
            "data": {"metadata": metadata_json(), "focus_resource_id": "resource-1", "depth": depth.unwrap_or(1), "nodes": [resource_json()], "edges": [], "frontier": [], "truncated": true}
        }),
        QueryRequest::GetHealthSummary => json!({
            "type": "get_health_summary",
            "data": {"metadata": metadata_json(), "resource_health": {"healthy": 0, "degraded": 0, "unhealthy": 0, "unknown": 0}, "freshness": {"fresh": 0, "stale": 0, "expired": 0}, "connector_health": {"healthy": 0, "degraded": 0, "auth_failed": 0, "rate_limited": 0, "unreachable": 0, "disabled": 0}}
        }),
        QueryRequest::GetRecentChanges(RecentChangesQuery { .. }) => json!({
            "type": "get_recent_changes",
            "data": {"metadata": metadata_json(), "items": [], "page_info": {"next_cursor": null}}
        }),
        QueryRequest::GetSyncStatus(SyncStatusQuery { .. }) => json!({
            "type": "get_sync_status",
            "data": {"metadata": metadata_json(), "connection": {"connection_id": "connection-1", "connector_type": "fixture", "display_name": "Fixture", "enabled": true, "health": "healthy", "last_success_at": null, "last_attempt_at": null}, "recent_runs": [], "next_scheduled_at": null}
        }),
        QueryRequest::ListConnectorCoverage => json!({
            "type": "list_connector_coverage",
            "data": {"metadata": metadata_json(), "items": []}
        }),
    };
    serde_json::from_value(value).unwrap()
}

fn metadata_json() -> serde_json::Value {
    json!({"schema_version": 1, "snapshot_version": "snapshot-1", "generated_at": "2026-08-04T00:00:00Z"})
}

fn resource_json() -> serde_json::Value {
    json!({"resource_id": "resource-1", "connection_id": "connection-1", "kind": "fixture", "display_name": "Fixture resource", "scope": "default", "lifecycle": "active", "health": "healthy", "freshness": "fresh", "observed_at": "2026-08-04T00:00:00Z"})
}
