//! MCP protocol projection boundary for Next Infra.

pub mod client;
pub mod input;
pub mod output;
pub mod server;

pub use client::{LocalRpcMcpClient, McpBridgeError, McpQueryClient};
pub use output::McpToolOutput;
pub use server::{CAPABILITIES_RESOURCE_URI, HEALTH_RESOURCE_URI, NextInfraMcp, TOOL_NAMES};
