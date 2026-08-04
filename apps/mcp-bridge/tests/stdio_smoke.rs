#![cfg(unix)]

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::thread;

use next_infra_local_rpc::protocol::{HostHello, QueryRequest, QueryResponse, RpcError};
use next_infra_local_rpc::session::{QueryHandler, RpcServer};
use next_infra_local_rpc::transport::{SecureUnixListener, TransportPaths};
use serde_json::{Value, json};
use tempfile::Builder;

#[test]
fn child_process_stdio_exposes_only_frozen_read_only_surface() {
    let home = Builder::new().prefix("ni").tempdir_in("/tmp").unwrap();
    let root = home
        .path()
        .join("Library")
        .join("Application Support")
        .join("Next Infra");
    fs::create_dir_all(&root).unwrap();
    let paths = TransportPaths::from_root(&root).unwrap();
    let listener = SecureUnixListener::bind(&paths).unwrap();
    let server = RpcServer::new(HostHello::initial("host", "0.1.0"), HealthHandler);
    let server_thread = thread::spawn(move || server.serve_once(&listener));

    let mut child = Command::new(env!("CARGO_BIN_EXE_next-infra-mcp"))
        .env("HOME", home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2026-07-28",
                "capabilities": {},
                "clientInfo": {"name": "next-infra-smoke", "version": "0.1.0"}
            }
        }),
    );
    let initialized = read_response(&mut reader, 1);
    assert!(initialized["result"]["capabilities"]["tools"].is_object());
    assert!(initialized["result"]["capabilities"]["resources"].is_object());

    send(
        &mut stdin,
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    );
    send(
        &mut stdin,
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}),
    );
    let tools = read_response(&mut reader, 2);
    let listed = tools["result"]["tools"].as_array().unwrap();
    assert_eq!(listed.len(), 7);
    for tool in listed {
        assert_eq!(tool["annotations"]["readOnlyHint"], true);
        assert_eq!(tool["annotations"]["destructiveHint"], false);
        assert_eq!(tool["annotations"]["idempotentHint"], true);
        assert_eq!(tool["annotations"]["openWorldHint"], false);
        assert!(tool["inputSchema"].is_object());
        assert!(tool["outputSchema"].is_object());
    }

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {"name": "get_health_summary", "arguments": {}}
        }),
    );
    let tool_result = read_response(&mut reader, 3);
    assert_eq!(
        tool_result["result"]["structuredContent"]["observed_at"],
        "2026-08-04T00:00:00Z"
    );
    assert_eq!(tool_result["result"]["isError"], false);

    send(
        &mut stdin,
        json!({"jsonrpc": "2.0", "id": 4, "method": "resources/list", "params": {}}),
    );
    let resources = read_response(&mut reader, 4);
    assert_eq!(
        resources["result"]["resources"].as_array().unwrap().len(),
        2
    );

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "resources/read",
            "params": {"uri": "next-infra://health-summary/v1"}
        }),
    );
    let resource = read_response(&mut reader, 5);
    let text = resource["result"]["contents"][0]["text"].as_str().unwrap();
    assert!(text.contains("observed_at"));

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "tools/call",
            "params": {"name": "get_resource", "arguments": {"resource_id": "missing", "include": []}}
        }),
    );
    let tool_error = read_response(&mut reader, 6);
    assert_eq!(tool_error["result"]["isError"], true);
    let error_text = tool_error["result"]["content"][0]["text"].as_str().unwrap();
    assert!(error_text.contains("query_failed"));
    assert!(!error_text.contains("/Users/"));
    assert!(!error_text.to_ascii_lowercase().contains("select "));

    drop(stdin);
    let status = child.wait().unwrap();
    assert!(status.success());
    let mut trailing = String::new();
    reader.read_to_string(&mut trailing).unwrap();
    for line in trailing.lines().filter(|line| !line.trim().is_empty()) {
        serde_json::from_str::<Value>(line).expect("stdout must contain only MCP JSON");
    }
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    assert!(stderr.is_empty(), "unexpected Bridge stderr: {stderr}");
    server_thread.join().unwrap().unwrap();
}

fn send(stdin: &mut impl Write, value: Value) {
    serde_json::to_writer(&mut *stdin, &value).unwrap();
    stdin.write_all(b"\n").unwrap();
    stdin.flush().unwrap();
}

fn read_response(reader: &mut impl BufRead, expected_id: u64) -> Value {
    loop {
        let mut line = String::new();
        assert_ne!(reader.read_line(&mut line).unwrap(), 0, "unexpected EOF");
        let value: Value = serde_json::from_str(line.trim()).expect("stdout must be JSON");
        if value.get("id").and_then(Value::as_u64) == Some(expected_id) {
            return value;
        }
    }
}

struct HealthHandler;

impl QueryHandler for HealthHandler {
    fn handle(&self, query: QueryRequest) -> Result<QueryResponse, RpcError> {
        if !matches!(query, QueryRequest::GetHealthSummary) {
            return Err(RpcError::query_failed(
                "The requested fixture resource is unavailable.",
                true,
            ));
        }
        serde_json::from_value(json!({
            "type": "get_health_summary",
            "data": {
                "metadata": {"schema_version": 1, "snapshot_version": "snapshot-1", "generated_at": "2026-08-04T00:00:00Z"},
                "resource_health": {"healthy": 1, "degraded": 0, "unhealthy": 0, "unknown": 0},
                "freshness": {"fresh": 1, "stale": 0, "expired": 0},
                "connector_health": {"healthy": 1, "degraded": 0, "auth_failed": 0, "rate_limited": 0, "unreachable": 0, "disabled": 0}
            }
        }))
        .map_err(|_| RpcError::query_failed("fixture response failed", false))
    }
}
