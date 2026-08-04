use std::fmt;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use next_infra_local_rpc::protocol::{
    Caller, QueryRequest, QueryResponse, RequestEnvelope, ResponseBody,
};
use next_infra_local_rpc::session::RpcClient;
use next_infra_local_rpc::transport::TransportPaths;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpBridgeError {
    pub code: &'static str,
    pub message: String,
    pub retryable: bool,
}

impl McpBridgeError {
    pub fn new(code: &'static str, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code,
            message: message.into(),
            retryable,
        }
    }

    pub fn safe_message(&self) -> String {
        format!("{}: {}", self.code, self.message)
    }
}

impl fmt::Display for McpBridgeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.safe_message())
    }
}

impl std::error::Error for McpBridgeError {}

pub trait McpQueryClient: Send + Sync + 'static {
    fn query(&self, query: QueryRequest) -> Result<QueryResponse, McpBridgeError>;
}

pub struct LocalRpcMcpClient {
    client: Mutex<RpcClient>,
    caller: Caller,
    next_request_id: AtomicU64,
}

impl LocalRpcMcpClient {
    pub fn new(
        client: RpcClient,
        bridge_version: impl Into<String>,
        release_id: impl Into<String>,
    ) -> Self {
        Self {
            client: Mutex::new(client),
            caller: Caller::bridge(bridge_version, release_id),
            next_request_id: AtomicU64::new(1),
        }
    }

    pub fn connect_run_dir(
        run_dir: impl Into<PathBuf>,
        bridge_version: impl Into<String>,
        release_id: impl Into<String>,
    ) -> Result<Self, McpBridgeError> {
        Self::connect_run_dir_with_timeout(
            run_dir,
            bridge_version,
            release_id,
            DEFAULT_CONNECT_TIMEOUT,
        )
    }

    pub fn connect_run_dir_with_timeout(
        run_dir: impl Into<PathBuf>,
        bridge_version: impl Into<String>,
        release_id: impl Into<String>,
        timeout: Duration,
    ) -> Result<Self, McpBridgeError> {
        let bridge_version = bridge_version.into();
        let release_id = release_id.into();
        let paths = TransportPaths::existing(run_dir).map_err(|_| {
            McpBridgeError::new(
                "host_unavailable",
                "Next Infra is not running or its local endpoint is unavailable.",
                true,
            )
        })?;
        let hello = next_infra_local_rpc::protocol::ClientHello::initial(
            bridge_version.clone(),
            release_id.clone(),
        );
        let client = RpcClient::connect_with_timeout(&paths, &hello, timeout).map_err(|_| {
            McpBridgeError::new(
                "host_unavailable",
                "Next Infra did not accept the local read-only session.",
                true,
            )
        })?;
        Ok(Self::new(client, bridge_version, release_id))
    }
}

const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

impl McpQueryClient for LocalRpcMcpClient {
    fn query(&self, query: QueryRequest) -> Result<QueryResponse, McpBridgeError> {
        let expected_capability = query.capability();
        let sequence = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let request = RequestEnvelope::new(format!("mcp-{sequence}"), self.caller.clone(), query)
            .map_err(|_| {
            McpBridgeError::new(
                "bridge_contract_error",
                "The local bridge could not construct a valid query request.",
                false,
            )
        })?;

        let mut client = self.client.lock().map_err(|_| {
            McpBridgeError::new(
                "host_unavailable",
                "The local Host session is unavailable. Reopen Next Infra and try again.",
                true,
            )
        })?;
        let response = client.query(&request).map_err(|_| {
            McpBridgeError::new(
                "host_unavailable",
                "The local Host did not complete the query. Reopen Next Infra and try again.",
                true,
            )
        })?;

        match response.body {
            ResponseBody::Query(response) if response.capability() == expected_capability => {
                Ok(*response)
            }
            ResponseBody::Query(_) => Err(McpBridgeError::new(
                "bridge_contract_error",
                "The local Host returned a response for a different query capability.",
                false,
            )),
            ResponseBody::Error(error) => Err(McpBridgeError::new(
                error.code.as_str(),
                error.message,
                error.retryable,
            )),
        }
    }
}
