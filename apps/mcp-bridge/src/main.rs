use std::path::PathBuf;
use std::process::ExitCode;

use next_infra_mcp::{LocalRpcMcpClient, NextInfraMcp, serve_stdio};

const APP_SUPPORT_SUFFIX: [&str; 4] = ["Library", "Application Support", "Next Infra", "run"];

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("next-infra-mcp: {message}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), String> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| String::from("host_unavailable: The user home directory is unavailable."))?;
    let mut run_dir = home;
    for component in APP_SUPPORT_SUFFIX {
        run_dir.push(component);
    }
    let bridge_version = env!("CARGO_PKG_VERSION");
    let release_id = option_env!("NEXT_INFRA_RELEASE_ID").unwrap_or(bridge_version);
    let client = LocalRpcMcpClient::connect_run_dir(run_dir, bridge_version, release_id)
        .map_err(|error| error.safe_message())?;
    serve_stdio(NextInfraMcp::new(client))
        .await
        .map_err(|_| String::from("bridge_io_error: The MCP STDIO session ended unexpectedly."))
}
