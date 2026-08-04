use next_infra_local_rpc::protocol::QueryResponse;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::client::McpBridgeError;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpToolOutput {
    pub observed_at: String,
    pub data: Value,
}

impl McpToolOutput {
    pub fn from_query_response(response: QueryResponse) -> Result<Self, McpBridgeError> {
        let observed_at = observed_at(&response).to_owned();
        let data = serde_json::to_value(response).map_err(|_| {
            McpBridgeError::new(
                "bridge_contract_error",
                "The local query result could not be projected to MCP output.",
                false,
            )
        })?;
        Ok(Self { observed_at, data })
    }
}

fn observed_at(response: &QueryResponse) -> &str {
    match response {
        QueryResponse::SearchResources(dto) => &dto.metadata.generated_at,
        QueryResponse::GetResource(dto) => &dto.metadata.generated_at,
        QueryResponse::GetTopology(dto) => &dto.metadata.generated_at,
        QueryResponse::GetHealthSummary(dto) => &dto.metadata.generated_at,
        QueryResponse::GetRecentChanges(dto) => &dto.metadata.generated_at,
        QueryResponse::GetSyncStatus(dto) => &dto.metadata.generated_at,
        QueryResponse::ListConnectorCoverage(dto) => &dto.metadata.generated_at,
    }
}
