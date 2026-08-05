//! Owner-only Local RPC listener owned by the Desktop Host process.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use next_infra_host_integration::{IntegrationPaths, UserQuitInspection, inspect_user_quit};
use next_infra_local_rpc::protocol::HostHello;
use next_infra_local_rpc::session::{QueryHandler, RpcServer};
use next_infra_local_rpc::transport::{SecureUnixListener, SocketError, TransportPaths};

use super::lifecycle::LaunchSource;

pub struct LocalRpcHost {
    stop: Arc<AtomicBool>,
    active_streams: Arc<Mutex<Vec<std::os::unix::net::UnixStream>>>,
    accept_thread: Option<JoinHandle<()>>,
}

impl LocalRpcHost {
    pub fn start<H>(
        paths: &IntegrationPaths,
        source: LaunchSource,
        handler: H,
    ) -> Result<Self, String>
    where
        H: QueryHandler,
    {
        if source == LaunchSource::McpAuthorized
            && inspect_user_quit(paths) != UserQuitInspection::Clear
        {
            return Err("local RPC authorization unavailable".into());
        }
        let transport =
            TransportPaths::from_root(&paths.root).map_err(|_| "local RPC path unavailable")?;
        let listener =
            SecureUnixListener::bind(&transport).map_err(|_| "local RPC listener unavailable")?;
        listener
            .listener()
            .set_nonblocking(true)
            .map_err(|_| "local RPC listener unavailable")?;

        let server = Arc::new(RpcServer::new(
            HostHello::initial(
                env!("CARGO_PKG_VERSION"),
                option_env!("NEXT_INFRA_RELEASE_ID").unwrap_or(env!("CARGO_PKG_VERSION")),
            ),
            handler,
        ));
        let stop = Arc::new(AtomicBool::new(false));
        let active_streams = Arc::new(Mutex::new(Vec::new()));
        let thread_stop = Arc::clone(&stop);
        let thread_streams = Arc::clone(&active_streams);
        let accept_thread = thread::spawn(move || {
            while !thread_stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        if thread_stop.load(Ordering::Acquire) {
                            let _ = stream.shutdown(std::net::Shutdown::Both);
                            break;
                        }
                        if stream.set_nonblocking(false).is_err() {
                            continue;
                        }
                        let shutdown_stream = match stream.try_clone() {
                            Ok(stream) => stream,
                            Err(_) => continue,
                        };
                        let Ok(mut active) = thread_streams.lock() else {
                            let _ = stream.shutdown(std::net::Shutdown::Both);
                            break;
                        };
                        active.push(shutdown_stream);
                        drop(active);
                        let server = Arc::clone(&server);
                        thread::spawn(move || {
                            let _ = server.serve_stream(stream);
                        });
                    }
                    Err(SocketError::Io(error))
                        if error.kind() == std::io::ErrorKind::WouldBlock =>
                    {
                        thread::sleep(Duration::from_millis(20));
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            stop,
            active_streams,
            accept_thread: Some(accept_thread),
        })
    }

    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Release);
        self.shutdown_active_streams();
        if let Some(thread) = self.accept_thread.take() {
            let _ = thread.join();
        }
    }

    fn shutdown_active_streams(&self) {
        if let Ok(mut streams) = self.active_streams.lock() {
            for stream in streams.drain(..) {
                let _ = stream.shutdown(std::net::Shutdown::Both);
            }
        }
    }
}

impl Drop for LocalRpcHost {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.shutdown_active_streams();
        if let Some(thread) = self.accept_thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use next_infra_host_integration::persist_user_quit;
    use next_infra_local_rpc::protocol::{
        Caller, ClientHello, QueryRequest, QueryResponse, RequestEnvelope, ResponseBody, RpcError,
    };
    use next_infra_local_rpc::session::{QueryHandler, RpcClient};
    use tempfile::{Builder, TempDir};

    use super::*;

    struct UnusedHandler;

    impl QueryHandler for UnusedHandler {
        fn handle(&self, _query: QueryRequest) -> Result<QueryResponse, RpcError> {
            Err(RpcError::query_failed("unused fixture", false))
        }
    }

    #[test]
    fn listener_accepts_a_real_owner_only_handshake_and_cleans_up() {
        let home = test_home();
        let paths = fixture_paths(home.path());
        let host =
            LocalRpcHost::start(&paths, LaunchSource::UserInteractive, UnusedHandler).unwrap();
        let transport = TransportPaths::existing(&paths.run_dir).unwrap();
        let mut client =
            RpcClient::connect(&transport, &ClientHello::initial("bridge-fixture", "0.1.0"))
                .unwrap();
        assert_eq!(client.host().release_id, "0.1.0");
        let response = client
            .query(
                &RequestEnvelope::new(
                    "desktop-host-query",
                    Caller::test("desktop-host-test"),
                    QueryRequest::GetHealthSummary,
                )
                .unwrap(),
            )
            .unwrap();
        assert!(matches!(response.body, ResponseBody::Error(_)));
        assert_eq!(
            fs::symlink_metadata(transport.socket_path())
                .unwrap()
                .permissions()
                .mode()
                & 0o7777,
            0o600
        );
        drop(client);
        host.stop();
        assert!(!transport.socket_path().exists());
    }

    #[test]
    fn mcp_start_rechecks_marker_before_binding() {
        let home = test_home();
        let paths = fixture_paths(home.path());
        persist_user_quit(&paths).unwrap();
        assert!(LocalRpcHost::start(&paths, LaunchSource::McpAuthorized, UnusedHandler).is_err());
        assert!(!paths.run_dir.join("next-infra-v1.sock").exists());
    }

    #[test]
    fn stopping_host_closes_active_bridge_sessions() {
        let home = test_home();
        let paths = fixture_paths(home.path());
        let host =
            LocalRpcHost::start(&paths, LaunchSource::UserInteractive, UnusedHandler).unwrap();
        let transport = TransportPaths::existing(&paths.run_dir).unwrap();
        let mut client =
            RpcClient::connect(&transport, &ClientHello::initial("bridge-fixture", "0.1.0"))
                .unwrap();
        host.stop();
        let request = RequestEnvelope::new(
            "after-stop",
            Caller::test("desktop-host-test"),
            QueryRequest::GetHealthSummary,
        )
        .unwrap();
        assert!(
            client
                .query_with_timeout(&request, Duration::from_millis(100))
                .is_err()
        );
    }

    fn fixture_paths(home: &std::path::Path) -> IntegrationPaths {
        let paths = IntegrationPaths::from_home(home);
        fs::create_dir_all(&paths.root).unwrap();
        fs::set_permissions(&paths.root, fs::Permissions::from_mode(0o700)).unwrap();
        paths
    }

    fn test_home() -> TempDir {
        Builder::new()
            .prefix("ni-desktop-rpc")
            .tempdir_in("/tmp")
            .unwrap()
    }
}
