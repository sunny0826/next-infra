//! Tauri composition root for the single local Control Plane instance.

use crate::adapter::{
    DesktopQueryAdapter, GetResourceCommand, GetTopologyCommand, LocalSettings,
    RecentChangesCommand, RuntimeCapabilities, SearchResourcesCommand, SyncStatusCommand,
    TimelineCommand, validate_settings_update,
};
use crate::host::authorization::authorize_launch;
use crate::host::lifecycle::LaunchSource;
use crate::host::local_rpc::LocalRpcHost;
use crate::scheduled_sync::{self, DriverHandle};
use next_infra_binding::{BindingInput, BindingService};
use next_infra_connector_aliyun::descriptor as aliyun_descriptor;
use next_infra_connector_api::{
    ConnectionInput, ReadConnector, SyncOutcome, SyncRequest, ValidationRequest, ValidationStatus,
};
use next_infra_connector_catalog::ConnectorCoverageSnapshot;
use next_infra_connector_cloudflare::cloudflare_descriptor;
use next_infra_connector_dokploy::dokploy_descriptor;
use next_infra_connector_github::{
    GitHubClient, GitHubConnector, GitHubEndpoint, GitHubFetch, GitHubFetchBudget, GitHubPages,
    ReqwestGitHubTransport, github_descriptor, repository::RepositoryDto,
};
use next_infra_connector_supabase_managed::descriptor as supabase_managed_descriptor;
use next_infra_connector_supabase_self_hosted::descriptor as supabase_self_hosted_descriptor;
use next_infra_connector_tencent::descriptor as tencent_descriptor;
use next_infra_core::{
    Connection, ConnectionId, ConnectorHealth, ConnectorType, DomainError, ErrorCode, ResourceKind,
    SchemaVersion, Scope, SecretValue, StoreReader, StoreWriter, SyncMode, SyncRunId, SyncTrigger,
    Timestamp,
};
use next_infra_host_integration::{
    IntegrationPaths, UserQuitInspection, authorize_mcp_host_launch, inspect_user_quit,
    persist_user_quit,
};
use next_infra_local_rpc::session::QueryServiceHandler;
use next_infra_normalizer::{AttributeSchema, Normalizer, RelationSchema};
use next_infra_query::dto::{
    BindingCommandResultDto, BindingDto, ChangePageDto, ConnectionSnapshotDto,
    ConnectorCoverageSnapshotDto, CreateBindingCommandDto, DisableBindingCommandDto, ErrorEnvelope,
    HealthSummaryDto, ResourceDetailDto, ResourcePageDto, SyncStatusDto, TimelinePageDto,
    TopologyDto, UpdateBindingCommandDto,
};
use next_infra_query::service::QueryService;
use next_infra_runtime::{
    CommittedQuerySource, ConnectorCatalogSnapshot, QueryContextRefreshHandle,
    QueryContextSnapshot, Runtime, ScheduledSync, Scheduler, SharedStore, SqliteRuntimeBackend,
};
use next_infra_sync::{SyncEngine, SyncRunStart};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, VecDeque};
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{App, AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_autostart::ManagerExt;

type DesktopRuntime = Runtime<SqliteRuntimeBackend, CommittedQuerySource>;

const DEFAULT_QUERY_SYNC_INTERVAL_MILLIS: u64 = 15 * 60 * 1_000;

pub struct AppState {
    runtime: Arc<Mutex<DesktopRuntime>>,
    store: SharedStore,
    query: DesktopQueryAdapter<CommittedQuerySource>,
    query_context: QueryContextRefreshHandle,
    settings: Mutex<LocalSettings>,
    settings_path: PathBuf,
    /// Shared GitHub connector instance (Arc-wrapped, not Clone). Reused across all syncs
    /// (scheduled, manual, connect-first-sync) so that `page_cache` (ETag + pages) and
    /// `route_cache` survive between runs within the same App lifetime — unchanged data hits
    /// If-None-Match → 304 → cached-page replay. App 重启后缓存丢失，首次同步全量 —— 已知边界。
    github_connector: Arc<GitHubConnector<ReqwestGitHubTransport>>,
    github_sync_running: Arc<AtomicBool>,
    integration_paths: IntegrationPaths,
    current_app_bundle: PathBuf,
    scheduler_driver: Mutex<Option<DriverHandle>>,
    pending_syncs: Arc<Mutex<VecDeque<ScheduledSync>>>,
    local_rpc: Mutex<Option<LocalRpcHost>>,
    explicit_quit: AtomicBool,
    system_shutdown: AtomicBool,
}

pub fn restore_main_window(app: &tauri::AppHandle) {
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    let window = app
        .get_webview_window("main")
        .or_else(|| build_main_window(app).ok());
    if let Some(window) = window {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

impl AppState {
    pub fn open(
        paths: &IntegrationPaths,
        source: LaunchSource,
        current_app_bundle: &Path,
    ) -> Result<Self, String> {
        let data_directory = &paths.root;
        ensure_data_directory(data_directory)?;
        let shared = SharedStore::open(&data_directory.join("next-infra.db"))
            .map_err(|_| "desktop store unavailable")?;
        let evaluated_at = now()?;
        let context =
            committed_query_context(&shared, evaluated_at, 0, &std::collections::BTreeMap::new())?;
        let query_source = CommittedQuerySource::new(shared.clone(), goal9_catalog(), context);
        let query_context = query_source.context_handle();
        let query = DesktopQueryAdapter::new(QueryService::new(query_source.clone()));
        let rpc_handler = QueryServiceHandler::new(QueryService::new(query_source.clone()));
        let backend = SqliteRuntimeBackend::from_shared_store(shared.clone());
        let mut runtime = Runtime::new(
            backend,
            QueryService::new(query_source),
            Scheduler::default(),
        );
        match source {
            LaunchSource::UserInteractive => runtime.start_interactive(evaluated_at),
            LaunchSource::LoginAutostart | LaunchSource::McpAuthorized => {
                runtime.start_background(evaluated_at)
            }
        }
        .map_err(|_| "desktop runtime unavailable")?;

        let local_rpc = match LocalRpcHost::start(paths, source, rpc_handler) {
            Ok(local_rpc) => local_rpc,
            Err(error) => {
                let _ = runtime.stop();
                return Err(error);
            }
        };

        let settings_path = data_directory.join("settings-v1.json");
        let settings = load_settings(&settings_path)?;
        let github_connector = Arc::new(github_connector()?);

        if let Ok(connections) = shared.read(|s| s.query_connections()) {
            for connection in connections.body {
                if connection.enabled && has_live_sync_path(&connection.connector_type) {
                    let interval = query_sync_interval_millis(&connection.connector_type);
                    let next_due = evaluated_at.unix_millis().saturating_add(interval as i64);
                    let next_due = Timestamp::from_unix_millis(next_due).unwrap_or(evaluated_at);
                    if let Err(e) = runtime.scheduler_mut().register(
                        connection.connection_id.clone(),
                        interval,
                        next_due,
                    ) {
                        eprintln!(
                            "scheduler: startup registration failed for {}: {:?}",
                            connection.connection_id.as_str(),
                            e
                        );
                    }
                }
            }
        }

        let store_for_driver = shared.clone();
        let runtime_for_driver = Arc::new(Mutex::new(runtime));
        let runtime_for_driver_clone = runtime_for_driver.clone();
        let pending_for_driver: Arc<Mutex<VecDeque<ScheduledSync>>> =
            Arc::new(Mutex::new(VecDeque::new()));
        let pending_for_driver_clone = pending_for_driver.clone();
        let running_for_driver = Arc::new(AtomicBool::new(false));
        let running_for_driver_clone = running_for_driver.clone();
        let store_for_driver_clone = store_for_driver.clone();
        let query_context_for_driver = query_context.clone();
        let github_connector_for_driver = github_connector.clone();

        let (tx, rx) = std::sync::mpsc::channel();
        let join = std::thread::spawn(move || {
            let rt = runtime_for_driver_clone;
            let store = store_for_driver_clone;
            let pending = pending_for_driver_clone;
            let running = running_for_driver_clone;
            let query_context = query_context_for_driver;
            loop {
                match rx.recv_timeout(Duration::from_millis(scheduled_sync::TICK_MILLIS)) {
                    Ok(()) => break,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        let github_connector = github_connector_for_driver.clone();
                        let Ok(at) = now() else {
                            continue;
                        };
                        // Collect the next_due map inside the same runtime lock that
                        // drives the tick, then drop the guard before any store read
                        // or context refresh (std::sync::Mutex is not reentrant).
                        let next_due: std::collections::BTreeMap<ConnectionId, Timestamp> = {
                            let Ok(mut guard) = rt.lock() else {
                                continue;
                            };
                            let pending_inner = &mut pending.lock().unwrap();
                            scheduled_sync::ScheduledSyncDriver::new(
                                {
                                    let store = store.clone();
                                    move |id: &ConnectionId| {
                                        store.read(|s| s.get_connection(id)).ok().flatten()
                                    }
                                },
                                {
                                    let store = store.clone();
                                    let running = running.clone();
                                    move |conn: Connection, trigger: SyncTrigger| {
                                        if running.swap(true, Ordering::AcqRel) {
                                            return Err(
                                                scheduled_sync::EnqueueError::SyncInProgress,
                                            );
                                        }
                                        let sync_run_id = SyncRunId::new(format!(
                                            "github-scheduled-{}",
                                            uuid::Uuid::new_v4()
                                        ))
                                        .map_err(|_| {
                                            running.store(false, Ordering::Release);
                                            scheduled_sync::EnqueueError::Unavailable
                                        })?;
                                        scheduled_sync::spawn_github_sync(
                                            store.clone(),
                                            running.clone(),
                                            github_connector.clone(),
                                            conn,
                                            trigger,
                                            sync_run_id,
                                        )
                                        .map_err(|_| {
                                            running.store(false, Ordering::Release);
                                            scheduled_sync::EnqueueError::Unavailable
                                        })?;
                                        Ok(())
                                    }
                                },
                            )
                            .tick(&mut guard, pending_inner, at);

                            guard
                                .scheduler()
                                .entries()
                                .map(|e| (e.connection_id.clone(), e.next_due_at))
                                .collect()
                        };

                        // Refresh query context after each tick so next_due reflects
                        // any scheduler state change (entries added, removed, or advanced).
                        if !next_due.is_empty() {
                            let revision = query_context.revision().unwrap_or(0).saturating_add(1);
                            let context = committed_query_context(&store, at, revision, &next_due);
                            if let Ok(ctx) = context {
                                let _ = query_context.refresh(ctx);
                            }
                        }
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        });
        let driver_handle = DriverHandle {
            stop: tx,
            join: Some(join),
        };

        let mut state = Self {
            runtime: runtime_for_driver,
            store: shared,
            query,
            query_context,
            settings: Mutex::new(settings),
            settings_path,
            github_connector,
            github_sync_running: running_for_driver,
            integration_paths: paths.clone(),
            current_app_bundle: current_app_bundle.to_path_buf(),
            scheduler_driver: Mutex::new(None),
            pending_syncs: pending_for_driver,
            local_rpc: Mutex::new(Some(local_rpc)),
            explicit_quit: AtomicBool::new(false),
            system_shutdown: AtomicBool::new(false),
        };

        // Initial context refresh so the first snapshot has real next_due values.
        let evaluated_at = now().map_err(|_| "desktop runtime unavailable")?;
        state
            .refresh_query_context(evaluated_at)
            .map_err(|_| "desktop runtime unavailable")?;

        state.scheduler_driver = Mutex::new(Some(driver_handle));
        Ok(state)
    }

    pub fn runtime(&self) -> &Mutex<DesktopRuntime> {
        &self.runtime
    }

    fn refresh_query_context(&self, evaluated_at: Timestamp) -> Result<(), ErrorEnvelope> {
        let revision = self
            .query_context
            .revision()
            .map_err(|_| {
                safe_error(
                    "query_context_unavailable",
                    "Local query context is unavailable.",
                )
            })?
            .checked_add(1)
            .ok_or_else(|| {
                safe_error(
                    "query_context_unavailable",
                    "Local query context is unavailable.",
                )
            })?;

        let next_due: std::collections::BTreeMap<ConnectionId, Timestamp> = self
            .runtime
            .lock()
            .map_err(|_| {
                safe_error(
                    "query_context_unavailable",
                    "Local query context is unavailable.",
                )
            })?
            .scheduler()
            .entries()
            .map(|e| (e.connection_id.clone(), e.next_due_at))
            .collect();

        let context = committed_query_context(&self.store, evaluated_at, revision, &next_due)
            .map_err(|_| {
                safe_error(
                    "query_context_unavailable",
                    "Local query context is unavailable.",
                )
            })?;
        self.query_context.refresh(context).map_err(|_| {
            safe_error(
                "query_context_unavailable",
                "Local query context is unavailable.",
            )
        })
    }

    fn user_quit_latched(&self) -> bool {
        inspect_user_quit(&self.integration_paths) != UserQuitInspection::Clear
    }

    fn runtime_capabilities(&self) -> RuntimeCapabilities {
        let user_quit = self.user_quit_latched();
        let configured =
            authorize_mcp_host_launch(&self.integration_paths, &self.current_app_bundle).is_ok();
        let reason = if user_quit {
            "Explicit Quit is latched. Reopen Next Infra interactively or at the next enabled login to clear suppression."
        } else if configured {
            "Trusted MCP integration is installed and authorized for this signed App."
        } else {
            "Trusted MCP integration is not installed, enabled, or verified for this App."
        };
        RuntimeCapabilities {
            start_at_login: true,
            manual_sync: true,
            mcp_auto_launch: configured,
            mcp_auto_launch_reason: reason.into(),
        }
    }

    pub fn persist_user_quit_and_stop(&self) -> Result<(), String> {
        if self.explicit_quit.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        if persist_user_quit(&self.integration_paths).is_err() {
            self.explicit_quit.store(false, Ordering::Release);
            return Err("user quit marker unavailable".into());
        }
        if let Some(local_rpc) = self
            .local_rpc
            .lock()
            .map_err(|_| "local RPC unavailable")?
            .take()
        {
            local_rpc.stop();
        }
        self.stop_scheduler_driver();
        self.runtime
            .lock()
            .map_err(|_| "desktop runtime unavailable")?
            .stop()
            .map_err(|_| "desktop runtime unavailable")?;
        Ok(())
    }

    fn stop_scheduler_driver(&self) {
        if let Some(handle) = self.scheduler_driver.lock().unwrap().take() {
            handle.stop();
        }
    }

    pub fn handle_sleep(&self) {
        if let Ok(mut runtime) = self.runtime.lock() {
            let _ = runtime.sleep();
        }
    }

    pub fn handle_wake(&self) {
        let Ok(at) = now() else { return };
        if let Ok(mut runtime) = self.runtime.lock()
            && let Ok(plans) = runtime.wake(at)
        {
            let mut pending = self.pending_syncs.lock().unwrap();
            pending.extend(plans);
        }
    }

    pub fn handle_power_off(&self) {
        self.system_shutdown.store(true, Ordering::Release);
        if let Ok(mut local_rpc) = self.local_rpc.lock()
            && let Some(local_rpc) = local_rpc.take()
        {
            local_rpc.stop();
        }
        self.stop_scheduler_driver();
        if let Ok(mut runtime) = self.runtime.lock() {
            let _ = runtime.stop();
        }
    }

    pub fn system_shutdown_requested(&self) -> bool {
        self.system_shutdown.load(Ordering::Acquire)
    }

    async fn create_github_connection(
        &self,
        request: GitHubConnectCommand,
    ) -> Result<GitHubConnectResult, ErrorEnvelope> {
        let display_name = request.display_name.trim();
        if display_name.is_empty() || display_name.len() > 120 {
            return Err(safe_error(
                "invalid_connection",
                "Connection name must contain between 1 and 120 characters.",
            ));
        }
        if request.token.trim().is_empty() || request.token.len() > 16 * 1024 {
            return Err(safe_error(
                "invalid_credential",
                "GitHub token is required.",
            ));
        }
        self.begin_github_sync()?;
        let result = async {
            let connection_id = ConnectionId::new(format!("github-{}", uuid::Uuid::new_v4()))
                .map_err(|_| {
                    safe_error(
                        "connection_unavailable",
                        "GitHub connection could not be created.",
                    )
                })?;
            if request.selected_repository_ids.is_empty() { return Err(safe_error("sync_scope_required", "Select at least one GitHub repository.")); }
            let input = github_connection_input(&connection_id, &request.selected_repository_ids);
            let secret = SecretValue::new(request.token.into_bytes());
            let connector = github_connector()
                .map_err(|e| safe_error("sync_unavailable", &e))?;
            let validation = connector
                .validate(
                    ValidationRequest {
                        connection: input.clone(),
                    },
                    Some(&secret),
                )
                .await
                .map_err(|_| {
                    safe_error(
                        "github_validation_failed",
                        "GitHub token could not be validated.",
                    )
                })?;
            if validation.status != ValidationStatus::Valid {
                return Err(validation_error(
                    validation.errors.first().map(|issue| issue.code),
                ));
            }

            let connection = Connection {
                connection_id: connection_id.clone(),
                connector_type: ConnectorType::new("github").expect("static connector type"),
                display_name: display_name.into(),
                enabled: true,
                config: serde_json::json!({ "selected_repository_ids": request.selected_repository_ids }),
                secret_ref: None,
                health: ConnectorHealth::Degraded,
                last_success_at: None,
                last_attempt_at: None,
                config_schema_version: SchemaVersion::new(1).expect("static schema version"),
                deleted_at: None,
            };
            if self
                .store
                .write(|store| store.upsert_connection(connection.clone()))
                .is_err()
            {
                return Err(safe_error(
                    "connection_unavailable",
                    "GitHub connection could not be saved.",
                ));
            }
            if self
                .store
                .write(|s| s.upsert_connection_secret(&connection_id, &secret))
                .is_err()
            {
                let _ = self.store.write(|s| s.remove_connection_secret(&connection_id));
                let _ = self.store.write(|s| s.purge_connection(&connection_id));
                return Err(safe_error(
                    "secret_storage_unavailable",
                    "GitHub token could not be stored locally.",
                ));
            }
            self.refresh_query_context(now().map_err(|_| {
                safe_error(
                    "query_context_unavailable",
                    "Local query context is unavailable.",
                )
            })?)?;
            if let Err(error) = self.register_github_connection(&connection) {
                eprintln!(
                    "scheduler: connection saved but registration failed: {}",
                    error.message
                );
            }
            let sync_run_id = self.enqueue_github_sync(connection, SyncTrigger::User)?;
            Ok(GitHubConnectResult {
                connection_id: connection_id.as_str().to_owned(),
                sync_run_id,
            })
        }
        .await;
        if result.is_err() {
            self.github_sync_running.store(false, Ordering::Release);
        }
        result
    }

    async fn manual_github_sync(&self, connection_id: String) -> Result<String, ErrorEnvelope> {
        let connection_id = ConnectionId::new(connection_id)
            .map_err(|_| safe_error("invalid_connection", "Connection identifier is invalid."))?;
        let connection = self
            .store
            .read(|store| store.get_connection(&connection_id))
            .map_err(|_| {
                safe_error(
                    "connection_unavailable",
                    "GitHub connection is unavailable.",
                )
            })?
            .filter(|connection| {
                connection.enabled && connection.connector_type.as_str() == "github"
            })
            .ok_or_else(|| {
                safe_error(
                    "connection_unavailable",
                    "GitHub connection is unavailable.",
                )
            })?;
        self.begin_github_sync()?;
        self.enqueue_github_sync(connection, SyncTrigger::User)
    }

    fn preview_github_connection_purge(
        &self,
        connection_id: String,
    ) -> Result<GitHubConnectionPurgeSummary, ErrorEnvelope> {
        let connection_id = self.github_connection_id(&connection_id)?;
        let summary = self
            .store
            .read(|store| store.preview_connection_purge(&connection_id))
            .map_err(|_| {
                safe_error(
                    "connection_purge_unavailable",
                    "The local connection snapshot could not be inspected.",
                )
            })?
            .ok_or_else(|| {
                safe_error(
                    "connection_unavailable",
                    "GitHub connection is unavailable.",
                )
            })?;
        Ok(summary.into())
    }

    fn query_github_actions_summary(&self) -> Result<GitHubActionsSummarySnapshot, ErrorEnvelope> {
        let rows = self
            .store
            .read(|store| store.query_github_actions_summary())
            .map_err(|_| {
                safe_error(
                    "github_actions_summary_unavailable",
                    "GitHub actions summary could not be read.",
                )
            })?;

        use std::collections::BTreeMap;
        let mut by_connection: BTreeMap<String, Vec<next_infra_store::GitHubActionsSummaryRow>> =
            BTreeMap::new();
        for row in rows.body {
            by_connection
                .entry(row.connection_id.as_str().to_owned())
                .or_default()
                .push(row);
        }

        let items: Vec<GitHubActionsSummary> = by_connection
            .into_iter()
            .map(|(connection_id, repo_rows)| {
                let connection_name = repo_rows
                    .first()
                    .map(|r| r.connection_name.as_str())
                    .unwrap_or("")
                    .to_owned();
                let repositories: Vec<GitHubRepositoryActions> = repo_rows
                    .into_iter()
                    .map(|r| GitHubRepositoryActions {
                        repository_id: r.repository_id.as_str().to_owned(),
                        repository_name: r.repository_name,
                        action_count: r.action_count,
                        succeeded: r.succeeded,
                        failed: r.failed,
                        running: r.running,
                    })
                    .collect();
                GitHubActionsSummary {
                    connection_id,
                    connection_name,
                    repositories,
                }
            })
            .collect();

        Ok(GitHubActionsSummarySnapshot { items })
    }

    fn purge_github_connection(
        &self,
        connection_id: String,
    ) -> Result<GitHubConnectionPurgeSummary, ErrorEnvelope> {
        if self.github_sync_running.swap(true, Ordering::AcqRel) {
            return Err(ErrorEnvelope {
                schema_version: next_infra_query::dto::QUERY_DTO_SCHEMA_VERSION,
                code: "sync_in_progress".into(),
                message: "A GitHub synchronization is already running.".into(),
                retryable: true,
            });
        }
        let result = (|| {
            let connection_id = self.github_connection_id(&connection_id)?;
            let summary = self
                .store
                .write(|store| store.purge_connection(&connection_id))
                .map_err(|_| {
                    safe_error(
                        "connection_purge_unavailable",
                        "The local connection snapshot could not be removed.",
                    )
                })?;
            self.runtime
                .lock()
                .map_err(|_| {
                    safe_error(
                        "scheduler_unavailable",
                        "Cannot remove scheduler entry: runtime unavailable.",
                    )
                })?
                .scheduler_mut()
                .remove(&connection_id);

            // The driver thread stays alive for the whole AppState lifetime
            // (it is a no-op while no entries are registered). It is stopped
            // only by explicit Quit or power-off; a later connection create
            // re-registers an entry without needing to restart the driver.
            self.refresh_query_context(now().map_err(|_| {
                safe_error(
                    "query_context_unavailable",
                    "Local query context is unavailable.",
                )
            })?)?;
            Ok(summary.into())
        })();
        self.github_sync_running.store(false, Ordering::Release);
        result
    }

    fn github_connection_id(&self, connection_id: &str) -> Result<ConnectionId, ErrorEnvelope> {
        let connection_id = ConnectionId::new(connection_id.to_owned())
            .map_err(|_| safe_error("invalid_connection", "Connection identifier is invalid."))?;
        self.store
            .read(|store| store.get_connection(&connection_id))
            .map_err(|_| {
                safe_error(
                    "connection_unavailable",
                    "GitHub connection is unavailable.",
                )
            })?
            .filter(|connection| connection.connector_type.as_str() == "github")
            .ok_or_else(|| {
                safe_error(
                    "connection_unavailable",
                    "GitHub connection is unavailable.",
                )
            })?;
        Ok(connection_id)
    }

    fn register_github_connection(&self, connection: &Connection) -> Result<(), ErrorEnvelope> {
        if !has_live_sync_path(&connection.connector_type) {
            return Ok(());
        }
        let at = now().map_err(|_| {
            safe_error(
                "scheduler_unavailable",
                "Cannot register connection: clock unavailable.",
            )
        })?;
        let interval = query_sync_interval_millis(&connection.connector_type);
        let next_due = at.unix_millis().saturating_add(interval as i64);
        let next_due = Timestamp::from_unix_millis(next_due).map_err(|_| {
            safe_error(
                "scheduler_unavailable",
                "Cannot register connection: timestamp overflow.",
            )
        })?;
        self.runtime
            .lock()
            .map_err(|_| {
                safe_error(
                    "scheduler_unavailable",
                    "Cannot register connection: runtime unavailable.",
                )
            })?
            .scheduler_mut()
            .register(connection.connection_id.clone(), interval, next_due)
            .map_err(|e| {
                safe_error(
                    "scheduler_error",
                    format!("Cannot register connection: {:?}", e).as_str(),
                )
            })?;
        Ok(())
    }

    fn begin_github_sync(&self) -> Result<(), ErrorEnvelope> {
        if self.github_sync_running.swap(true, Ordering::AcqRel) {
            return Err(ErrorEnvelope {
                schema_version: next_infra_query::dto::QUERY_DTO_SCHEMA_VERSION,
                code: "sync_in_progress".into(),
                message: "A GitHub synchronization is already running.".into(),
                retryable: true,
            });
        }
        Ok(())
    }

    fn enqueue_github_sync(
        &self,
        connection: Connection,
        trigger: SyncTrigger,
    ) -> Result<String, ErrorEnvelope> {
        let sync_run_id =
            SyncRunId::new(format!("github-sync-{}", uuid::Uuid::new_v4())).map_err(|_| {
                safe_error(
                    "sync_unavailable",
                    "GitHub synchronization could not start.",
                )
            })?;
        scheduled_sync::spawn_github_sync(
            self.store.clone(),
            self.github_sync_running.clone(),
            self.github_connector.clone(),
            connection,
            trigger,
            sync_run_id,
        )
    }
}

pub(crate) async fn sync_github(
    store: SharedStore,
    connector: Arc<GitHubConnector<ReqwestGitHubTransport>>,
    mut connection: Connection,
    trigger: SyncTrigger,
    sync_run_id: SyncRunId,
) -> Result<(), ErrorEnvelope> {
    let started_at = now().map_err(|_| {
        safe_error(
            "sync_unavailable",
            "GitHub synchronization could not start.",
        )
    })?;
    connection.last_attempt_at = Some(started_at);
    store
        .write(|store| store.upsert_connection(connection.clone()))
        .map_err(|_| {
            safe_error(
                "sync_unavailable",
                "GitHub synchronization could not start.",
            )
        })?;
    let request = SyncRequest {
        sync_run_id: sync_run_id.clone(),
        connection: ConnectionInput {
            connection_id: connection.connection_id.clone(),
            connector_type: ConnectorType::new("github").expect("static connector type"),
            config: connection.config.clone(),
            config_schema_version: SchemaVersion::new(1).expect("static schema version"),
        },
        mode: SyncMode::Full,
        scope: Scope::new(format!("github:{}", connection.connection_id.as_str())).map_err(
            |_| {
                safe_error(
                    "sync_unavailable",
                    "GitHub synchronization could not start.",
                )
            },
        )?,
        cursor: None,
        targeted_resources: Vec::new(),
    };
    let secret = store
        .read(|s| s.connection_secret(&connection.connection_id))
        .map_err(|_| {
            safe_error(
                "credential_unavailable",
                "GitHub token is unavailable from local storage.",
            )
        })?
        .ok_or_else(|| {
            safe_error(
                "credential_unavailable",
                "GitHub token is unavailable from local storage.",
            )
        })?;
    let mut engine = SyncEngine::new(store.clone());
    let handle = engine
        .start(
            &connection,
            SyncRunStart {
                sync_run_id: sync_run_id.clone(),
                mode: SyncMode::Full,
                trigger,
                scope: request.scope.clone(),
                started_at,
                targeted_resources: Vec::new(),
            },
        )
        .map_err(|_| {
            safe_error(
                "sync_unavailable",
                "GitHub synchronization could not start.",
            )
        })?;
    let outcome = connector.sync(request.clone(), Some(&secret)).await;
    let finished_at = now().map_err(|_| {
        safe_error(
            "sync_unavailable",
            "GitHub synchronization could not finish.",
        )
    })?;

    let health = match outcome {
        Ok(outcome) => {
            let health = if matches!(outcome, SyncOutcome::Partial { .. }) {
                ConnectorHealth::Degraded
            } else {
                ConnectorHealth::Healthy
            };
            let normalized = match github_normalizer().normalize(&request, outcome.batch().clone())
            {
                Ok(normalized) => normalized,
                Err(_) => {
                    let _ = engine.fail(
                        handle,
                        DomainError {
                            code: ErrorCode::Internal,
                            message: "GitHub data normalization failed.".into(),
                            retryable: false,
                        },
                        finished_at,
                    );
                    return Err(safe_error(
                        "sync_unavailable",
                        "GitHub data could not be saved.",
                    ));
                }
            };
            engine
                .commit(handle, normalized, finished_at)
                .map_err(|_| safe_error("sync_unavailable", "GitHub data could not be saved."))?;
            connection.last_success_at = Some(finished_at);
            health
        }
        Err(failure) => {
            let health = connector_health(failure.code);
            engine
                .fail(
                    handle,
                    DomainError {
                        code: failure.code,
                        message: "GitHub synchronization failed.".into(),
                        retryable: failure.retryable,
                    },
                    finished_at,
                )
                .map_err(|_| {
                    safe_error(
                        "sync_unavailable",
                        "GitHub synchronization could not finish.",
                    )
                })?;
            connection.health = health;
            store
                .write(|store| store.upsert_connection(connection))
                .map_err(|_| {
                    safe_error(
                        "connection_unavailable",
                        "GitHub connection status could not be saved.",
                    )
                })?;
            return Err(sync_error(failure.code));
        }
    };
    connection.health = health;
    store
        .write(|store| store.upsert_connection(connection))
        .map_err(|_| {
            safe_error(
                "connection_unavailable",
                "GitHub connection status could not be saved.",
            )
        })?;
    Ok(())
}

fn goal9_catalog() -> ConnectorCatalogSnapshot {
    let descriptors = [
        github_descriptor(),
        dokploy_descriptor(),
        cloudflare_descriptor(),
        supabase_managed_descriptor(),
        supabase_self_hosted_descriptor(),
        aliyun_descriptor(),
        tencent_descriptor(),
    ];
    ConnectorCatalogSnapshot::from(
        descriptors
            .into_iter()
            .filter_map(|descriptor| ConnectorCoverageSnapshot::from_descriptor(&descriptor).ok())
            .collect::<Vec<_>>(),
    )
}

pub fn setup(
    app: &mut App,
    paths: &IntegrationPaths,
    source: LaunchSource,
    current_app_bundle: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if source == LaunchSource::McpAuthorized {
        authorize_launch(source, paths, current_app_bundle)
            .map_err(|_| std::io::Error::other("desktop launch authorization changed"))?;
    }
    let state = AppState::open(paths, source, current_app_bundle).map_err(std::io::Error::other)?;
    app.manage(state);
    crate::host::effects::install_workspace_observers(app.app_handle())
        .map_err(std::io::Error::other)?;
    let show = MenuItem::with_id(app, "show", "Show Next Infra", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Next Infra", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    let mut tray = TrayIconBuilder::new().menu(&menu).tooltip("Next Infra");
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.on_menu_event(|app, event| match event.id.as_ref() {
        "show" => {
            restore_main_window(app);
        }
        "quit" => {
            if let Some(state) = app.try_state::<AppState>()
                && state.persist_user_quit_and_stop().is_ok()
            {
                app.exit(0);
            }
        }
        _ => {}
    })
    .build(app)?;
    if source == LaunchSource::UserInteractive {
        app.set_activation_policy(tauri::ActivationPolicy::Regular);
        build_main_window(app.app_handle())?;
    } else {
        app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    }
    Ok(())
}

fn build_main_window(app: &AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("Next Infra")
        .inner_size(900.0, 600.0)
        .build()
}

fn ensure_data_directory(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink()
                || !metadata.is_dir()
                || metadata.uid() != unsafe { libc::geteuid() as u32 }
                || metadata.permissions().mode() & 0o7777 != 0o700
            {
                return Err("desktop data directory unavailable".into());
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(path).map_err(|_| "desktop data directory unavailable")?;
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))
                .map_err(|_| "desktop data directory unavailable")?;
        }
        Err(_) => return Err("desktop data directory unavailable".into()),
    }
    Ok(())
}

pub fn invoke_handler() -> impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        query_list_connections,
        query_search_resources,
        query_get_resource,
        query_get_topology,
        query_health_summary,
        query_recent_changes,
        query_timeline,
        binding_create,
        binding_update,
        binding_disable,
        query_sync_status,
        query_connector_coverage,
        query_github_actions_summary,
        github_discover_repositories,
        github_connect,
        github_connection_purge_preview,
        github_connection_purge,
        runtime_manual_sync,
        local_settings_get,
        local_settings_update,
        runtime_capabilities,
    ]
}

#[derive(Deserialize)]
struct GitHubConnectCommand {
    display_name: String,
    token: String,
    selected_repository_ids: Vec<String>,
}

#[derive(Deserialize)]
struct GitHubRepositoryDiscoveryCommand {
    token: String,
}

#[derive(Serialize)]
struct GitHubRepositoryOption {
    id: String,
    name: String,
}

#[derive(Serialize)]
struct GitHubConnectResult {
    connection_id: String,
    sync_run_id: String,
}

#[derive(Deserialize)]
struct GitHubConnectionPurgeCommand {
    connection_id: String,
}

#[derive(Serialize)]
struct GitHubConnectionPurgeSummary {
    resources: u64,
    relations: u64,
    resource_versions: u64,
    relation_versions: u64,
    changes: u64,
    bindings: u64,
    sync_runs: u64,
}

impl From<next_infra_store::ConnectionPurgeSummary> for GitHubConnectionPurgeSummary {
    fn from(value: next_infra_store::ConnectionPurgeSummary) -> Self {
        Self {
            resources: value.resources,
            relations: value.relations,
            resource_versions: value.resource_versions,
            relation_versions: value.relation_versions,
            changes: value.changes,
            bindings: value.bindings,
            sync_runs: value.sync_runs,
        }
    }
}

#[derive(Serialize)]
struct GitHubActionsSummarySnapshot {
    items: Vec<GitHubActionsSummary>,
}

#[derive(Serialize)]
struct GitHubActionsSummary {
    connection_id: String,
    connection_name: String,
    repositories: Vec<GitHubRepositoryActions>,
}

#[derive(Serialize)]
struct GitHubRepositoryActions {
    repository_id: String,
    repository_name: String,
    action_count: u64,
    succeeded: u64,
    failed: u64,
    running: u64,
}

#[tauri::command]
async fn github_connect(
    state: State<'_, AppState>,
    request: GitHubConnectCommand,
) -> Result<GitHubConnectResult, ErrorEnvelope> {
    state.create_github_connection(request).await
}

#[tauri::command]
async fn github_discover_repositories(
    request: GitHubRepositoryDiscoveryCommand,
) -> Result<Vec<GitHubRepositoryOption>, ErrorEnvelope> {
    let secret = SecretValue::new(request.token.into_bytes());
    let client = GitHubClient::new(
        ReqwestGitHubTransport::new()
            .map_err(|_| safe_error("github_unavailable", "GitHub is unavailable."))?,
    );
    let endpoint = GitHubEndpoint::new(
        "repository_discovery",
        "/user/repos",
        &[
            ("visibility", "all"),
            ("affiliation", "owner,collaborator,organization_member"),
            ("sort", "full_name"),
            ("direction", "asc"),
            ("per_page", "100"),
        ],
    )
    .map_err(|_| safe_error("github_unavailable", "GitHub is unavailable."))?;
    let pages = match client
        .fetch_pages_with_budget(
            &endpoint,
            &secret,
            None,
            GitHubFetchBudget::new(1, 1).expect("static budget"),
        )
        .await
    {
        Ok(GitHubFetch::Pages(pages)) => pages,
        Ok(GitHubFetch::NotModified { .. }) => {
            return Err(safe_error(
                "github_validation_failed",
                "GitHub repositories could not be loaded.",
            ));
        }
        Err(failure)
            if failure.failure.code == ErrorCode::PartialPagination && failure.is_partial() =>
        {
            // Discovery budgets a single page (up to 100 repositories). An
            // account with more accessible repositories trips the budget;
            // the pages already fetched are exactly what discovery promises.
            GitHubPages {
                pages: failure.completed_pages,
                etag: None,
                request_summary: failure.request_summary,
            }
        }
        Err(_) => {
            return Err(safe_error(
                "github_validation_failed",
                "GitHub repositories could not be loaded.",
            ));
        }
    };
    let repositories = pages
        .pages
        .into_iter()
        .map(|page| page.deserialize::<Vec<RepositoryDto>>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| {
            safe_error(
                "github_validation_failed",
                "GitHub repositories could not be loaded.",
            )
        })?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    Ok(repositories
        .into_iter()
        .take(100)
        .map(|repo| GitHubRepositoryOption {
            id: repo.id.to_string(),
            name: format!("{}/{}", repo.owner.login, repo.name),
        })
        .collect())
}

#[tauri::command]
fn github_connection_purge_preview(
    state: State<'_, AppState>,
    request: GitHubConnectionPurgeCommand,
) -> Result<GitHubConnectionPurgeSummary, ErrorEnvelope> {
    state.preview_github_connection_purge(request.connection_id)
}

#[tauri::command]
fn query_github_actions_summary(
    state: State<'_, AppState>,
) -> Result<GitHubActionsSummarySnapshot, ErrorEnvelope> {
    state.query_github_actions_summary()
}

#[tauri::command]
fn github_connection_purge(
    state: State<'_, AppState>,
    request: GitHubConnectionPurgeCommand,
) -> Result<GitHubConnectionPurgeSummary, ErrorEnvelope> {
    state.purge_github_connection(request.connection_id)
}

#[tauri::command]
fn query_list_connections(
    state: State<'_, AppState>,
) -> Result<ConnectionSnapshotDto, ErrorEnvelope> {
    state.query.list_connections()
}

#[tauri::command]
fn query_search_resources(
    state: State<'_, AppState>,
    request: SearchResourcesCommand,
) -> Result<ResourcePageDto, ErrorEnvelope> {
    state.query.search_resources(request)
}

#[tauri::command]
fn query_get_resource(
    state: State<'_, AppState>,
    request: GetResourceCommand,
) -> Result<ResourceDetailDto, ErrorEnvelope> {
    state.query.get_resource(request)
}

#[tauri::command]
fn query_get_topology(
    state: State<'_, AppState>,
    request: GetTopologyCommand,
) -> Result<TopologyDto, ErrorEnvelope> {
    state.query.get_topology(request)
}

#[tauri::command]
fn query_health_summary(state: State<'_, AppState>) -> Result<HealthSummaryDto, ErrorEnvelope> {
    state.query.get_health_summary()
}

#[tauri::command]
fn query_recent_changes(
    state: State<'_, AppState>,
    request: RecentChangesCommand,
) -> Result<ChangePageDto, ErrorEnvelope> {
    state.query.get_recent_changes(request)
}

#[tauri::command]
fn query_timeline(
    state: State<'_, AppState>,
    request: TimelineCommand,
) -> Result<TimelinePageDto, ErrorEnvelope> {
    state.query.get_timeline(request)
}

#[tauri::command]
fn binding_create(
    state: State<'_, AppState>,
    request: CreateBindingCommandDto,
) -> Result<BindingCommandResultDto, ErrorEnvelope> {
    let input = binding_input(
        request.source_resource_id,
        request.target_resource_id,
        request.kind,
    )?;
    let binding = state
        .store
        .write(|store| {
            BindingService::new(store)
                .create(
                    input,
                    now().map_err(|_| {
                        next_infra_store::StoreError::Contract("clock unavailable".into())
                    })?,
                )
                .map_err(binding_store_error)
        })
        .map_err(|_| binding_error("binding_unavailable", "Binding could not be saved.", true))?;
    binding_result(&state, binding)
}

#[tauri::command]
fn binding_update(
    state: State<'_, AppState>,
    request: UpdateBindingCommandDto,
) -> Result<BindingCommandResultDto, ErrorEnvelope> {
    let binding_id = next_infra_core::BindingId::new(request.binding_id)
        .map_err(|_| binding_error("invalid_binding", "Binding identifier is invalid.", false))?;
    let input = binding_input(
        request.source_resource_id,
        request.target_resource_id,
        request.kind,
    )?;
    let binding = state
        .store
        .write(|store| {
            BindingService::new(store)
                .update(
                    &binding_id,
                    input,
                    now().map_err(|_| {
                        next_infra_store::StoreError::Contract("clock unavailable".into())
                    })?,
                )
                .map_err(binding_store_error)
        })
        .map_err(|_| binding_error("binding_unavailable", "Binding could not be saved.", true))?;
    binding_result(&state, binding)
}

#[tauri::command]
fn binding_disable(
    state: State<'_, AppState>,
    request: DisableBindingCommandDto,
) -> Result<BindingCommandResultDto, ErrorEnvelope> {
    let binding_id = next_infra_core::BindingId::new(request.binding_id)
        .map_err(|_| binding_error("invalid_binding", "Binding identifier is invalid.", false))?;
    let binding = state
        .store
        .write(|store| {
            BindingService::new(store)
                .disable(
                    &binding_id,
                    now().map_err(|_| {
                        next_infra_store::StoreError::Contract("clock unavailable".into())
                    })?,
                )
                .map_err(binding_store_error)
        })
        .map_err(|_| binding_error("binding_unavailable", "Binding could not be saved.", true))?;
    binding_result(&state, binding)
}

fn binding_input(
    source_resource_id: String,
    target_resource_id: String,
    kind: String,
) -> Result<BindingInput, ErrorEnvelope> {
    Ok(BindingInput {
        source_resource_id: next_infra_core::ResourceId::new(source_resource_id)
            .map_err(|_| binding_error("invalid_binding", "Binding source is invalid.", false))?,
        target_resource_id: next_infra_core::ResourceId::new(target_resource_id)
            .map_err(|_| binding_error("invalid_binding", "Binding target is invalid.", false))?,
        kind: next_infra_core::RelationKind::new(kind)
            .map_err(|_| binding_error("invalid_binding", "Binding kind is invalid.", false))?,
    })
}

fn binding_store_error(
    error: next_infra_binding::BindingError<next_infra_store::StoreError>,
) -> next_infra_store::StoreError {
    next_infra_store::StoreError::Contract(error.to_string())
}

fn binding_result(
    state: &AppState,
    binding: next_infra_core::Binding,
) -> Result<BindingCommandResultDto, ErrorEnvelope> {
    let metadata = state.query.list_connections()?.metadata;
    Ok(BindingCommandResultDto {
        metadata,
        binding: BindingDto {
            binding_id: binding.binding_id.as_str().to_owned(),
            source_resource_id: binding.source_resource_id.as_str().to_owned(),
            target_resource_id: binding.target_resource_id.as_str().to_owned(),
            kind: binding.kind.as_str().to_owned(),
            status: match binding.status {
                next_infra_core::BindingStatus::Active => {
                    next_infra_query::dto::BindingStatusDto::Active
                }
                next_infra_core::BindingStatus::Unresolved => {
                    next_infra_query::dto::BindingStatusDto::Unresolved
                }
                next_infra_core::BindingStatus::Disabled => {
                    next_infra_query::dto::BindingStatusDto::Disabled
                }
            },
            created_at: next_infra_runtime::format_timestamp(binding.created_at),
            updated_at: next_infra_runtime::format_timestamp(binding.updated_at),
        },
    })
}

fn binding_error(code: &str, message: &str, retryable: bool) -> ErrorEnvelope {
    ErrorEnvelope {
        schema_version: next_infra_query::dto::QUERY_DTO_SCHEMA_VERSION,
        code: code.into(),
        message: message.into(),
        retryable,
    }
}

#[tauri::command]
fn query_sync_status(
    state: State<'_, AppState>,
    request: SyncStatusCommand,
) -> Result<SyncStatusDto, ErrorEnvelope> {
    state.query.get_sync_status(request)
}

#[tauri::command]
fn query_connector_coverage(
    state: State<'_, AppState>,
) -> Result<ConnectorCoverageSnapshotDto, ErrorEnvelope> {
    state.query.list_connector_coverage()
}

#[tauri::command]
async fn runtime_manual_sync(
    state: State<'_, AppState>,
    connection_id: String,
) -> Result<crate::adapter::ManualSyncResult, ErrorEnvelope> {
    state
        .manual_github_sync(connection_id)
        .await
        .map(|sync_run_id| crate::adapter::ManualSyncResult { sync_run_id })
}

fn github_connection_input(
    connection_id: &ConnectionId,
    selected_repository_ids: &[String],
) -> ConnectionInput {
    ConnectionInput {
        connection_id: connection_id.clone(),
        connector_type: ConnectorType::new("github").expect("static connector type"),
        config: serde_json::json!({ "selected_repository_ids": selected_repository_ids }),
        config_schema_version: SchemaVersion::new(1).expect("static schema version"),
    }
}

fn github_connector() -> Result<GitHubConnector<ReqwestGitHubTransport>, String> {
    ReqwestGitHubTransport::new()
        .map(GitHubConnector::new)
        .map_err(|_| "GitHub transport is unavailable.".to_string())
}

fn validation_error(code: Option<ErrorCode>) -> ErrorEnvelope {
    let health = code
        .map(connector_health)
        .unwrap_or(ConnectorHealth::Degraded);
    let (code, message, retryable) = match health {
        ConnectorHealth::AuthFailed => (
            "github_auth_failed",
            "GitHub token was not accepted.",
            false,
        ),
        ConnectorHealth::RateLimited => (
            "github_rate_limited",
            "GitHub validation is rate limited. Try again later.",
            true,
        ),
        ConnectorHealth::Unreachable => (
            "github_unreachable",
            "GitHub could not be reached. Try again later.",
            true,
        ),
        _ => (
            "github_validation_failed",
            "GitHub token could not be validated.",
            false,
        ),
    };
    ErrorEnvelope {
        schema_version: next_infra_query::dto::QUERY_DTO_SCHEMA_VERSION,
        code: code.into(),
        message: message.into(),
        retryable,
    }
}

fn sync_error(code: ErrorCode) -> ErrorEnvelope {
    let health = connector_health(code);
    let (code, message, retryable) = match health {
        ConnectorHealth::AuthFailed => (
            "github_auth_failed",
            "GitHub token was not accepted.",
            false,
        ),
        ConnectorHealth::RateLimited => (
            "github_rate_limited",
            "GitHub is rate limited. Try again later.",
            true,
        ),
        ConnectorHealth::Unreachable => (
            "github_unreachable",
            "GitHub could not be reached. Try again later.",
            true,
        ),
        _ => (
            "github_sync_failed",
            "GitHub synchronization failed.",
            false,
        ),
    };
    ErrorEnvelope {
        schema_version: next_infra_query::dto::QUERY_DTO_SCHEMA_VERSION,
        code: code.into(),
        message: message.into(),
        retryable,
    }
}

fn connector_health(code: ErrorCode) -> ConnectorHealth {
    match code {
        ErrorCode::AuthenticationFailed | ErrorCode::CredentialUnavailable => {
            ConnectorHealth::AuthFailed
        }
        ErrorCode::RateLimited => ConnectorHealth::RateLimited,
        ErrorCode::NetworkUnreachable | ErrorCode::ProviderUnavailable => {
            ConnectorHealth::Unreachable
        }
        _ => ConnectorHealth::Degraded,
    }
}

fn github_normalizer() -> Normalizer {
    Normalizer::new(
        [
            github_schema(
                "github.repository",
                &[
                    "repository_id",
                    "visibility",
                    "default_branch",
                    "archived",
                    "disabled",
                    "created_at",
                    "updated_at",
                ],
            ),
            github_schema(
                "github.workflow",
                &["workflow_id", "path", "state", "created_at", "updated_at"],
            ),
            github_schema(
                "github.workflow_run",
                &[
                    "run_id",
                    "workflow_id",
                    "run_number",
                    "status",
                    "conclusion",
                    "created_at",
                ],
            ),
        ],
        [
            github_relation("github.contains", "github.repository", "github.workflow"),
            github_relation("github.executes", "github.workflow", "github.workflow_run"),
        ],
    )
    .expect("static GitHub schemas are valid")
}

fn github_schema(kind: &str, fields: &[&str]) -> AttributeSchema {
    AttributeSchema {
        kind: ResourceKind::new(kind).expect("static resource kind"),
        schema_version: SchemaVersion::new(1).expect("static schema version"),
        allowed_attributes: fields
            .iter()
            .map(|field| (*field).into())
            .collect::<BTreeSet<_>>(),
    }
}

fn github_relation(kind: &str, source: &str, target: &str) -> RelationSchema {
    RelationSchema {
        kind: next_infra_core::RelationKind::new(kind).expect("static relation kind"),
        source_kind: ResourceKind::new(source).expect("static resource kind"),
        target_kind: ResourceKind::new(target).expect("static resource kind"),
    }
}

#[tauri::command]
fn local_settings_get(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<LocalSettings, ErrorEnvelope> {
    let mut settings = state
        .settings
        .lock()
        .map(|settings| settings.clone())
        .map_err(|_| safe_error("settings_unavailable", "Local settings are unavailable."))?;
    settings.start_at_login = app
        .autolaunch()
        .is_enabled()
        .map_err(|_| safe_error("autostart_unavailable", "Start at login is unavailable."))?;
    settings.user_quit = state.user_quit_latched();
    Ok(settings)
}

#[tauri::command]
fn local_settings_update(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: LocalSettings,
) -> Result<LocalSettings, ErrorEnvelope> {
    let mut current = state
        .settings
        .lock()
        .map_err(|_| safe_error("settings_unavailable", "Local settings are unavailable."))?;
    current.user_quit = state.user_quit_latched();
    let updated = validate_settings_update(&current, settings)?;
    let autostart = app.autolaunch();
    if updated.start_at_login {
        autostart.enable()
    } else {
        autostart.disable()
    }
    .map_err(|_| {
        safe_error(
            "autostart_unavailable",
            "Start at login could not be changed.",
        )
    })?;
    persist_settings(&state.settings_path, &updated)?;
    *current = updated.clone();
    Ok(updated)
}

#[tauri::command]
fn runtime_capabilities(state: State<'_, AppState>) -> RuntimeCapabilities {
    state.runtime_capabilities()
}

fn load_settings(path: &Path) -> Result<LocalSettings, String> {
    match fs::read(path) {
        Ok(bytes) => {
            serde_json::from_slice(&bytes).map_err(|_| "local settings are invalid".into())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(LocalSettings::default()),
        Err(_) => Err("local settings are unavailable".into()),
    }
}

fn persist_settings(path: &Path, settings: &LocalSettings) -> Result<(), ErrorEnvelope> {
    let temporary = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec(settings)
        .map_err(|_| safe_error("settings_unavailable", "Local settings could not be saved."))?;
    fs::write(&temporary, bytes)
        .and_then(|_| fs::rename(&temporary, path))
        .map_err(|_| safe_error("settings_unavailable", "Local settings could not be saved."))
}

fn now() -> Result<Timestamp, String> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "system clock unavailable")?
        .as_millis();
    let millis = i64::try_from(millis).map_err(|_| "system clock unavailable")?;
    Timestamp::from_unix_millis(millis).map_err(|_| "system clock unavailable".into())
}

fn committed_query_context(
    store: &SharedStore,
    evaluated_at: Timestamp,
    query_context_revision: u64,
    next_due: &std::collections::BTreeMap<ConnectionId, Timestamp>,
) -> Result<QueryContextSnapshot, String> {
    let connections = store
        .read(|store| store.query_connections())
        .map_err(|_| "desktop query context unavailable")?;
    QueryContextSnapshot::from_intervals(
        evaluated_at,
        query_context_revision,
        connections.body.into_iter().map(|connection| {
            (
                connection.connection_id.clone(),
                query_sync_interval_millis(&connection.connector_type),
                next_due.get(&connection.connection_id).cloned(),
            )
        }),
    )
    .map_err(|_| "desktop query context unavailable".into())
}

/// True only for "github" today — single source of truth per plan §2.2.
/// Other connectors use offline replay/fixtures and are NOT scheduled.
pub(crate) fn has_live_sync_path(connector_type: &ConnectorType) -> bool {
    connector_type.as_str() == "github"
}

pub(crate) fn query_sync_interval_millis(connector_type: &ConnectorType) -> u64 {
    if connector_type.as_str() == "github" {
        return github_descriptor()
            .recommended_sync_interval_secs
            .saturating_mul(1_000);
    }
    DEFAULT_QUERY_SYNC_INTERVAL_MILLIS
}

fn safe_error(code: &str, message: &str) -> ErrorEnvelope {
    ErrorEnvelope {
        schema_version: next_infra_query::dto::QUERY_DTO_SCHEMA_VERSION,
        code: code.into(),
        message: message.into(),
        retryable: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::{Builder, TempDir};

    #[test]
    fn catalog_registers_goal9_modules_for_desktop_queries() {
        let catalog = goal9_catalog();
        let modules = catalog
            .connectors
            .iter()
            .flat_map(|connector| {
                connector
                    .modules
                    .iter()
                    .map(|module| module.module.as_str())
            })
            .collect::<Vec<_>>();
        for expected in [
            "supabase.managed.projects",
            "supabase.self_hosted.service_api",
            "aliyun.network.security_group",
            "tencent.edge.public_ip",
        ] {
            assert!(modules.contains(&expected), "missing {expected}");
        }
    }

    #[test]
    fn composition_opens_one_runtime_and_persists_safe_settings() {
        let directory = test_home();
        let paths = IntegrationPaths::from_home(directory.path());
        let state =
            AppState::open(&paths, LaunchSource::UserInteractive, &paths.stable_app).unwrap();
        assert!(matches!(
            state.runtime().lock().unwrap().state(),
            next_infra_runtime::RuntimeState::Running(_)
        ));
        let updated = LocalSettings {
            data_budget_mb: 1024,
            retention_days: 60,
            ..LocalSettings::default()
        };
        persist_settings(&state.settings_path, &updated).unwrap();
        assert_eq!(load_settings(&state.settings_path).unwrap(), updated);
        state.persist_user_quit_and_stop().unwrap();
        state.persist_user_quit_and_stop().unwrap();
        assert!(state.integration_paths.user_quit.exists());
        assert_eq!(
            state.runtime().lock().unwrap().state(),
            next_infra_runtime::RuntimeState::Stopped
        );
    }

    #[test]
    fn background_sources_start_runtime_without_needing_a_window() {
        for source in [LaunchSource::LoginAutostart, LaunchSource::McpAuthorized] {
            let directory = test_home();
            let paths = IntegrationPaths::from_home(directory.path());
            let state = AppState::open(&paths, source, &paths.stable_app).unwrap();
            assert!(matches!(
                state.runtime().lock().unwrap().state(),
                next_infra_runtime::RuntimeState::Running(_)
            ));
        }
    }

    #[test]
    fn composition_rehydrates_query_context_for_persisted_connections() {
        let directory = test_home();
        let paths = IntegrationPaths::from_home(directory.path());
        ensure_data_directory(&paths.root).unwrap();
        let store = SharedStore::open(&paths.root.join("next-infra.db")).unwrap();
        let connection_id = ConnectionId::new("github-persisted-connection").unwrap();
        store
            .write(|store| {
                store.upsert_connection(Connection {
                    connection_id: connection_id.clone(),
                    connector_type: ConnectorType::new("github").unwrap(),
                    display_name: "Persisted GitHub".into(),
                    enabled: true,
                    config: serde_json::json!({"selected_repository_ids": ["42"]}),
                    secret_ref: None,
                    health: ConnectorHealth::Degraded,
                    last_success_at: None,
                    last_attempt_at: None,
                    config_schema_version: SchemaVersion::new(1).unwrap(),
                    deleted_at: None,
                })
            })
            .unwrap();
        drop(store);

        let state =
            AppState::open(&paths, LaunchSource::UserInteractive, &paths.stable_app).unwrap();
        let status = state
            .query
            .get_sync_status(SyncStatusCommand {
                connection_id: connection_id.as_str().into(),
                recent_run_limit: Some(1),
            })
            .unwrap();

        assert_eq!(status.connection.connection_id, connection_id.as_str());
        assert!(
            status.next_scheduled_at.is_some()
                && !status.next_scheduled_at.as_ref().unwrap().is_empty(),
            "next_scheduled_at should be a real time from the scheduler"
        );
        let rt = state.runtime().lock().unwrap();
        let entry = rt.scheduler().entry(&connection_id).unwrap();
        let current_time = now().unwrap();
        let tolerance = 5_000;
        let expected_next = current_time.unix_millis() + 900_000;
        assert!(
            (entry.next_due_at.unix_millis() - expected_next).abs() < tolerance,
            "scheduler next_due_at should be within tolerance of now+900_000"
        );
        drop(rt);
        assert!(state.query.get_health_summary().is_ok());
    }

    #[test]
    fn github_connection_purge_removes_the_scoped_snapshot_and_credential() {
        let directory = test_home();
        let paths = IntegrationPaths::from_home(directory.path());
        let state =
            AppState::open(&paths, LaunchSource::UserInteractive, &paths.stable_app).unwrap();
        let connection_id = ConnectionId::new("github-purge-fixture").unwrap();
        state
            .store
            .write(|store| {
                store.upsert_connection(Connection {
                    connection_id: connection_id.clone(),
                    connector_type: ConnectorType::new("github").unwrap(),
                    display_name: "Purge fixture".into(),
                    enabled: true,
                    config: serde_json::json!({"selected_repository_ids": ["42"]}),
                    secret_ref: None,
                    health: ConnectorHealth::Degraded,
                    last_success_at: None,
                    last_attempt_at: None,
                    config_schema_version: SchemaVersion::new(1).unwrap(),
                    deleted_at: None,
                })
            })
            .unwrap();
        state
            .store
            .write(|s| {
                s.upsert_connection_secret(&connection_id, &SecretValue::new("fixture-token"))
            })
            .unwrap();

        let preview = state
            .preview_github_connection_purge(connection_id.as_str().into())
            .unwrap();
        assert_eq!(preview.resources, 0);
        let result = state
            .purge_github_connection(connection_id.as_str().into())
            .unwrap();

        assert_eq!(result.resources, 0);
        assert_eq!(state.store.get_connection(&connection_id).unwrap(), None);
        assert!(
            state
                .store
                .read(|s| s.connection_secret(&connection_id))
                .unwrap()
                .is_none()
        );
        assert!(state.query.list_connections().unwrap().items.is_empty());
    }

    #[test]
    fn host_mcp_state_reports_unavailable_and_explicit_quit_without_clearing_it() {
        let directory = test_home();
        let paths = IntegrationPaths::from_home(directory.path());
        let state =
            AppState::open(&paths, LaunchSource::UserInteractive, &paths.stable_app).unwrap();
        let unavailable = state.runtime_capabilities();
        assert!(!unavailable.mcp_auto_launch);
        assert!(unavailable.mcp_auto_launch_reason.contains("not installed"));

        persist_user_quit(&paths).unwrap();
        let suppressed = state.runtime_capabilities();
        assert!(!suppressed.mcp_auto_launch);
        assert!(suppressed.mcp_auto_launch_reason.contains("Explicit Quit"));
        assert!(state.user_quit_latched());
    }

    #[test]
    fn system_power_off_stops_without_latching_user_quit() {
        let directory = test_home();
        let paths = IntegrationPaths::from_home(directory.path());
        let state =
            AppState::open(&paths, LaunchSource::UserInteractive, &paths.stable_app).unwrap();
        state.handle_power_off();
        assert!(state.system_shutdown_requested());
        assert!(!paths.user_quit.exists());
        assert_eq!(
            state.runtime().lock().unwrap().state(),
            next_infra_runtime::RuntimeState::Stopped
        );
    }

    #[test]
    fn binding_commands_reject_malformed_identifiers_without_exposing_store_errors() {
        let error = binding_input(
            " ".into(),
            "fixture-target".into(),
            "infra.depends_on".into(),
        )
        .unwrap_err();
        assert_eq!(error.code, "invalid_binding");
        assert!(!error.retryable);
        assert_eq!(error.message, "Binding source is invalid.");

        let error = binding_error("binding_unavailable", "Binding could not be saved.", true);
        assert_eq!(error.code, "binding_unavailable");
        assert!(error.retryable);
        assert!(!error.message.contains("StoreError"));
    }

    // ── Scheduler integration tests ─────────────────────────────────────────

    #[test]
    fn scheduler_restarts_existing_github_connections_on_open() {
        let directory = test_home();
        let paths = IntegrationPaths::from_home(directory.path());
        let conn_id = ConnectionId::new("github-seeded-conn").unwrap();
        let store = SharedStore::open(&paths.root.join("next-infra.db")).unwrap();
        store
            .write(|s| {
                s.upsert_connection(Connection {
                    connection_id: conn_id.clone(),
                    connector_type: ConnectorType::new("github").unwrap(),
                    display_name: "Seeded".into(),
                    enabled: true,
                    config: serde_json::json!({"selected_repository_ids": ["42"]}),
                    secret_ref: None,
                    health: ConnectorHealth::Healthy,
                    last_success_at: None,
                    last_attempt_at: None,
                    config_schema_version: SchemaVersion::new(1).unwrap(),
                    deleted_at: None,
                })
            })
            .unwrap();
        drop(store);

        let state =
            AppState::open(&paths, LaunchSource::UserInteractive, &paths.stable_app).unwrap();

        let rt = state.runtime().lock().unwrap();
        let entry = rt.scheduler().entry(&conn_id);
        assert!(
            entry.is_some(),
            "github connection should be registered on open"
        );
        let entry = entry.unwrap();
        assert_eq!(entry.interval_millis, 900_000);
        let current_time = now().unwrap();
        let tolerance = 5_000;
        let expected_next = current_time.unix_millis() + 900_000;
        assert!(
            (entry.next_due_at.unix_millis() - expected_next).abs() < tolerance,
            "next_due_at should be within tolerance of now+900_000"
        );
        drop(rt);
        assert!(
            state.scheduler_driver.lock().unwrap().is_some(),
            "driver should be running for existing github connection"
        );
        state.persist_user_quit_and_stop().unwrap();
    }

    #[test]
    fn scheduler_disabled_github_connection_not_registered() {
        let directory = test_home();
        let paths = IntegrationPaths::from_home(directory.path());
        let conn_id = ConnectionId::new("github-disabled-conn").unwrap();
        let store = SharedStore::open(&paths.root.join("next-infra.db")).unwrap();
        store
            .write(|s| {
                s.upsert_connection(Connection {
                    connection_id: conn_id.clone(),
                    connector_type: ConnectorType::new("github").unwrap(),
                    display_name: "Disabled".into(),
                    enabled: false,
                    config: serde_json::json!({"selected_repository_ids": ["42"]}),
                    secret_ref: None,
                    health: ConnectorHealth::Healthy,
                    last_success_at: None,
                    last_attempt_at: None,
                    config_schema_version: SchemaVersion::new(1).unwrap(),
                    deleted_at: None,
                })
            })
            .unwrap();
        drop(store);

        let state =
            AppState::open(&paths, LaunchSource::UserInteractive, &paths.stable_app).unwrap();

        let rt = state.runtime().lock().unwrap();
        assert!(
            rt.scheduler().entry(&conn_id).is_none(),
            "disabled github connection should not be registered"
        );
        drop(rt);
        assert!(
            state.scheduler_driver.lock().unwrap().is_some(),
            "driver starts regardless of connection state"
        );
        state.persist_user_quit_and_stop().unwrap();
    }

    #[test]
    fn scheduler_non_github_connection_not_registered() {
        let directory = test_home();
        let paths = IntegrationPaths::from_home(directory.path());
        let conn_id = ConnectionId::new("ssh-conn").unwrap();
        let store = SharedStore::open(&paths.root.join("next-infra.db")).unwrap();
        store
            .write(|s| {
                s.upsert_connection(Connection {
                    connection_id: conn_id.clone(),
                    connector_type: ConnectorType::new("ssh").unwrap(),
                    display_name: "SSH".into(),
                    enabled: true,
                    config: serde_json::json!({}),
                    secret_ref: None,
                    health: ConnectorHealth::Healthy,
                    last_success_at: None,
                    last_attempt_at: None,
                    config_schema_version: SchemaVersion::new(1).unwrap(),
                    deleted_at: None,
                })
            })
            .unwrap();
        drop(store);

        let state =
            AppState::open(&paths, LaunchSource::UserInteractive, &paths.stable_app).unwrap();

        let rt = state.runtime().lock().unwrap();
        assert!(
            rt.scheduler().entry(&conn_id).is_none(),
            "ssh connection should not be registered"
        );
        drop(rt);
        assert!(
            state.scheduler_driver.lock().unwrap().is_some(),
            "driver starts regardless of connection state"
        );
        state.persist_user_quit_and_stop().unwrap();
    }

    #[test]
    fn purge_removes_scheduler_entry_and_keeps_driver_alive() {
        let directory = test_home();
        let paths = IntegrationPaths::from_home(directory.path());
        let conn_id = ConnectionId::new("github-purge-conn").unwrap();
        let store = SharedStore::open(&paths.root.join("next-infra.db")).unwrap();
        store
            .write(|s| {
                s.upsert_connection(Connection {
                    connection_id: conn_id.clone(),
                    connector_type: ConnectorType::new("github").unwrap(),
                    display_name: "Purge Me".into(),
                    enabled: true,
                    config: serde_json::json!({"selected_repository_ids": ["42"]}),
                    secret_ref: None,
                    health: ConnectorHealth::Healthy,
                    last_success_at: None,
                    last_attempt_at: None,
                    config_schema_version: SchemaVersion::new(1).unwrap(),
                    deleted_at: None,
                })
            })
            .unwrap();
        drop(store);

        let state =
            AppState::open(&paths, LaunchSource::UserInteractive, &paths.stable_app).unwrap();

        assert!(
            state.scheduler_driver.lock().unwrap().is_some(),
            "driver should be running"
        );

        state.purge_github_connection(conn_id.to_string()).unwrap();

        assert!(
            state
                .runtime()
                .lock()
                .unwrap()
                .scheduler()
                .entry(&conn_id)
                .is_none(),
            "scheduler entry should be removed after purge"
        );
        assert!(
            state.scheduler_driver.lock().unwrap().is_some(),
            "driver must stay alive so a later connection create can be scheduled"
        );
        state.persist_user_quit_and_stop().unwrap();
    }

    #[test]
    fn explicit_quit_stops_scheduler_driver() {
        let directory = test_home();
        let paths = IntegrationPaths::from_home(directory.path());
        let conn_id = ConnectionId::new("github-quit-conn").unwrap();
        let store = SharedStore::open(&paths.root.join("next-infra.db")).unwrap();
        store
            .write(|s| {
                s.upsert_connection(Connection {
                    connection_id: conn_id.clone(),
                    connector_type: ConnectorType::new("github").unwrap(),
                    display_name: "Quit Test".into(),
                    enabled: true,
                    config: serde_json::json!({"selected_repository_ids": ["42"]}),
                    secret_ref: None,
                    health: ConnectorHealth::Healthy,
                    last_success_at: None,
                    last_attempt_at: None,
                    config_schema_version: SchemaVersion::new(1).unwrap(),
                    deleted_at: None,
                })
            })
            .unwrap();
        drop(store);

        let state =
            AppState::open(&paths, LaunchSource::UserInteractive, &paths.stable_app).unwrap();

        assert!(
            state.scheduler_driver.lock().unwrap().is_some(),
            "driver should be running"
        );

        persist_user_quit(&paths).unwrap();
        state.stop_scheduler_driver();

        assert!(
            state.scheduler_driver.lock().unwrap().is_none(),
            "driver should be stopped after quit"
        );
        state.persist_user_quit_and_stop().unwrap();
    }

    fn test_home() -> TempDir {
        Builder::new()
            .prefix("ni-desktop-composition")
            .tempdir_in("/tmp")
            .unwrap()
    }
}
