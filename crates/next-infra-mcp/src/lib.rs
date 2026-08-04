//! MCP protocol projection boundary for Next Infra.

pub mod client;
pub mod input;
pub mod output;
pub mod server;

pub use client::{LocalRpcMcpClient, McpBridgeError, McpQueryClient};
pub use output::McpToolOutput;
pub use server::{CAPABILITIES_RESOURCE_URI, HEALTH_RESOURCE_URI, NextInfraMcp, TOOL_NAMES};

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
