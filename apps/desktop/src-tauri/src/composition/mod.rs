//! Tauri composition root for the single local Control Plane instance.

use crate::adapter::{
    DesktopQueryAdapter, GetResourceCommand, GetTopologyCommand, LocalSettings,
    RecentChangesCommand, RuntimeCapabilities, SearchResourcesCommand, SyncStatusCommand,
    manual_sync_unavailable, validate_settings_update,
};
use next_infra_core::Timestamp;
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
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{App, AppHandle, Manager, State};
use tauri_plugin_autostart::ManagerExt;

type DesktopRuntime = Runtime<SqliteRuntimeBackend, CommittedQuerySource>;

pub struct AppState {
    runtime: Mutex<DesktopRuntime>,
    query: DesktopQueryAdapter<CommittedQuerySource>,
    settings: Mutex<LocalSettings>,
    settings_path: PathBuf,
    user_quit_path: PathBuf,
}

pub fn restore_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

impl AppState {
    pub fn open(data_directory: &Path) -> Result<Self, String> {
        fs::create_dir_all(data_directory).map_err(|_| "desktop data directory unavailable")?;
        let shared = SharedStore::open(&data_directory.join("next-infra.db"))
            .map_err(|_| "desktop store unavailable")?;
        let evaluated_at = now()?;
        let context = QueryContextSnapshot::empty(evaluated_at, 0);
        let source =
            CommittedQuerySource::new(shared.clone(), ConnectorCatalogSnapshot::default(), context);
        let query = DesktopQueryAdapter::new(QueryService::new(source.clone()));
        let backend = SqliteRuntimeBackend::from_shared_store(shared);
        let mut runtime = Runtime::new(backend, QueryService::new(source), Scheduler::default());
        runtime
            .start_interactive(evaluated_at)
            .map_err(|_| "desktop runtime unavailable")?;

        let settings_path = data_directory.join("settings-v1.json");
        let settings = load_settings(&settings_path)?;
        Ok(Self {
            runtime: Mutex::new(runtime),
            query,
            settings: Mutex::new(settings),
            settings_path,
            user_quit_path: data_directory.join("state").join("user-quit-v1.json"),
        })
    }

    pub fn runtime(&self) -> &Mutex<DesktopRuntime> {
        &self.runtime
    }

    pub fn persist_user_quit_and_stop(&self) -> Result<(), String> {
        let parent = self
            .user_quit_path
            .parent()
            .ok_or("user quit path unavailable")?;
        fs::create_dir_all(parent).map_err(|_| "user quit marker unavailable")?;
        let temporary = self.user_quit_path.with_extension("json.tmp");
        fs::write(&temporary, br#"{"schema_version":1,"user_quit":true}"#)
            .and_then(|_| fs::rename(&temporary, &self.user_quit_path))
            .map_err(|_| "user quit marker unavailable")?;
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
        if let Ok(mut runtime) = self.runtime.lock() {
            let _ = runtime.stop();
        }
    }
}

pub fn setup(app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    let data_directory = app.path().app_data_dir()?;
    let state = AppState::open(&data_directory).map_err(std::io::Error::other)?;
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
    use tempfile::TempDir;

    #[test]
    fn composition_opens_one_runtime_and_persists_safe_settings() {
        let directory = TempDir::new().unwrap();
        let state = AppState::open(directory.path()).unwrap();
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
        assert!(state.user_quit_path.exists());
        assert_eq!(
            state.runtime().lock().unwrap().state(),
            next_infra_runtime::RuntimeState::Stopped
        );
    }
}
