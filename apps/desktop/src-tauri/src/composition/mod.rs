//! Tauri composition root for the single local Control Plane instance.

use crate::adapter::{
    DesktopQueryAdapter, GetResourceCommand, GetTopologyCommand, LocalSettings,
    RecentChangesCommand, RuntimeCapabilities, SearchResourcesCommand, SyncStatusCommand,
    manual_sync_unavailable, validate_settings_update,
};
use crate::host::authorization::authorize_launch;
use crate::host::lifecycle::LaunchSource;
use crate::host::local_rpc::LocalRpcHost;
use next_infra_core::Timestamp;
use next_infra_host_integration::{IntegrationPaths, persist_user_quit};
use next_infra_local_rpc::session::QueryServiceHandler;
use next_infra_query::dto::{
    ChangePageDto, ConnectionSnapshotDto, ConnectorCoverageSnapshotDto, ErrorEnvelope,
    HealthSummaryDto, ResourceDetailDto, ResourcePageDto, SyncStatusDto, TopologyDto,
};
use next_infra_query::service::QueryService;
use next_infra_runtime::{
    CommittedQuerySource, ConnectorCatalogSnapshot, QueryContextSnapshot, Runtime, Scheduler,
    SharedStore, SqliteRuntimeBackend,
};
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{App, AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_autostart::ManagerExt;

type DesktopRuntime = Runtime<SqliteRuntimeBackend, CommittedQuerySource>;

pub struct AppState {
    runtime: Mutex<DesktopRuntime>,
    query: DesktopQueryAdapter<CommittedQuerySource>,
    settings: Mutex<LocalSettings>,
    settings_path: PathBuf,
    integration_paths: IntegrationPaths,
    local_rpc: Mutex<Option<LocalRpcHost>>,
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
    pub fn open(paths: &IntegrationPaths, source: LaunchSource) -> Result<Self, String> {
        let data_directory = &paths.root;
        ensure_data_directory(data_directory)?;
        let shared = SharedStore::open(&data_directory.join("next-infra.db"))
            .map_err(|_| "desktop store unavailable")?;
        let evaluated_at = now()?;
        let context = QueryContextSnapshot::empty(evaluated_at, 0);
        let query_source =
            CommittedQuerySource::new(shared.clone(), ConnectorCatalogSnapshot::default(), context);
        let query = DesktopQueryAdapter::new(QueryService::new(query_source.clone()));
        let rpc_handler = QueryServiceHandler::new(QueryService::new(query_source.clone()));
        let backend = SqliteRuntimeBackend::from_shared_store(shared);
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
        Ok(Self {
            runtime: Mutex::new(runtime),
            query,
            settings: Mutex::new(settings),
            settings_path,
            integration_paths: paths.clone(),
            local_rpc: Mutex::new(Some(local_rpc)),
        })
    }

    pub fn runtime(&self) -> &Mutex<DesktopRuntime> {
        &self.runtime
    }

    pub fn persist_user_quit_and_stop(&self) -> Result<(), String> {
        persist_user_quit(&self.integration_paths).map_err(|_| "user quit marker unavailable")?;
        if let Some(local_rpc) = self
            .local_rpc
            .lock()
            .map_err(|_| "local RPC unavailable")?
            .take()
        {
            local_rpc.stop();
        }
        self.runtime
            .lock()
            .map_err(|_| "desktop runtime unavailable")?
            .stop()
            .map_err(|_| "desktop runtime unavailable")?;
        Ok(())
    }

    pub fn handle_sleep(&self) {
        if let Ok(mut runtime) = self.runtime.lock() {
            let _ = runtime.sleep();
        }
    }

    pub fn handle_wake(&self) {
        let Ok(at) = now() else { return };
        if let Ok(mut runtime) = self.runtime.lock() {
            let _ = runtime.wake(at);
        }
    }

    pub fn handle_power_off(&self) {
        if let Ok(mut local_rpc) = self.local_rpc.lock()
            && let Some(local_rpc) = local_rpc.take()
        {
            local_rpc.stop();
        }
        if let Ok(mut runtime) = self.runtime.lock() {
            let _ = runtime.stop();
        }
    }
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
    let state = AppState::open(paths, source).map_err(std::io::Error::other)?;
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
        query_sync_status,
        query_connector_coverage,
        runtime_manual_sync,
        local_settings_get,
        local_settings_update,
        runtime_capabilities,
    ]
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
fn runtime_manual_sync(
    _state: State<'_, AppState>,
    connection_id: String,
) -> Result<crate::adapter::ManualSyncResult, ErrorEnvelope> {
    let _ = connection_id;
    manual_sync_unavailable()
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
fn runtime_capabilities(_state: State<'_, AppState>) -> RuntimeCapabilities {
    RuntimeCapabilities {
        start_at_login: true,
        manual_sync: false,
        mcp_auto_launch: false,
    }
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
    fn composition_opens_one_runtime_and_persists_safe_settings() {
        let directory = test_home();
        let paths = IntegrationPaths::from_home(directory.path());
        let state = AppState::open(&paths, LaunchSource::UserInteractive).unwrap();
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
            let state = AppState::open(&paths, source).unwrap();
            assert!(matches!(
                state.runtime().lock().unwrap().state(),
                next_infra_runtime::RuntimeState::Running(_)
            ));
        }
    }

    fn test_home() -> TempDir {
        Builder::new()
            .prefix("ni-desktop-composition")
            .tempdir_in("/tmp")
            .unwrap()
    }
}
