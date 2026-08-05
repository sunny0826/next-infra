use std::path::PathBuf;
use std::process::ExitCode;

use next_infra_mcp::{NextInfraMcp, serve_stdio};
use next_infra_mcp_bridge::availability::{
    AvailabilityPolicy, IntegrationPaths, LocalRpcConnector, MacCodeSignatureVerifier,
    MacOpenLauncher, SystemMonotonicClock, ensure_host,
};

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
    let paths = IntegrationPaths::from_home(&home);
    let bridge_version = env!("CARGO_PKG_VERSION");
    let release_id = option_env!("NEXT_INFRA_RELEASE_ID").unwrap_or(bridge_version);
    let current_executable = std::env::current_exe().map_err(|_| {
        String::from("host_unavailable: The Bridge executable path is unavailable.")
    })?;
    let connector = LocalRpcConnector::new(
        paths.run_dir.clone(),
        bridge_version.to_owned(),
        release_id.to_owned(),
    );
    let verifier = MacCodeSignatureVerifier;
    let launcher = MacOpenLauncher::default();
    let clock = SystemMonotonicClock::default();
    let client = ensure_host(
        &paths,
        &current_executable,
        &connector,
        &verifier,
        &launcher,
        &clock,
        AvailabilityPolicy::default(),
    )
    .map_err(|error| error.safe_message())?;
    serve_stdio(NextInfraMcp::new(client))
        .await
        .map_err(|_| String::from("bridge_io_error: The MCP STDIO session ended unexpectedly."))
}
