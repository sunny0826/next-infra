//! MCP protocol projection boundary for Next Infra.

pub mod client;
pub mod input;
pub mod output;
pub mod server;

pub use client::{LocalRpcMcpClient, McpBridgeError, McpQueryClient};
pub use output::McpToolOutput;
pub use server::{CAPABILITIES_RESOURCE_URI, HEALTH_RESOURCE_URI, NextInfraMcp, TOOL_NAMES};

pub const LOCAL_RPC_PROTOCOL_MAJOR: u16 = next_infra_local_rpc::protocol::PROTOCOL_MAJOR;
pub const LOCAL_RPC_PROTOCOL_MINOR: u16 = next_infra_local_rpc::protocol::PROTOCOL_MINOR;
pub const LOCAL_RPC_MINIMUM_SUPPORTED_MINOR: u16 =
    next_infra_local_rpc::protocol::MINIMUM_SUPPORTED_MINOR;

pub fn local_rpc_capability_names() -> Vec<&'static str> {
    next_infra_local_rpc::protocol::Capability::ALL
        .into_iter()
        .map(next_infra_local_rpc::protocol::Capability::as_str)
        .collect()
}

pub async fn serve_stdio<C>(server: NextInfraMcp<C>) -> Result<(), String>
where
    C: McpQueryClient,
{
    use rmcp::ServiceExt;

    let service = server
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|_| String::from("MCP client initialization failed."))?;
    service
        .waiting()
        .await
        .map_err(|_| String::from("MCP STDIO worker stopped unexpectedly."))?;
    Ok(())
}
