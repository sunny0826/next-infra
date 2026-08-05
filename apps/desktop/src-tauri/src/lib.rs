//! Desktop Host modules composed by the `next-infra` binary.

use std::ffi::OsString;
use std::path::PathBuf;

use host::authorization::{app_bundle_from_executable, authorize_launch, parse_process_arguments};
use host::lifecycle::LaunchSource;
use next_infra_host_integration::IntegrationPaths;
use tauri::Manager;

pub mod adapter;
pub mod composition;
pub mod host;
pub mod keychain;

pub fn run() -> Result<(), String> {
    let source = parse_process_arguments(std::env::args_os())
        .map_err(|_| String::from("desktop_launch_rejected: Invalid launch arguments."))?;
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| String::from("desktop_launch_rejected: HOME is unavailable."))?;
    let paths = IntegrationPaths::from_home(&home);
    let current_executable = std::env::current_exe()
        .map_err(|_| String::from("desktop_launch_rejected: Executable path is unavailable."))?;
    let current_app = app_bundle_from_executable(&current_executable)
        .map(PathBuf::from)
        .or_else(|| (source != LaunchSource::McpAuthorized).then(|| paths.stable_app.clone()))
        .ok_or_else(|| String::from("desktop_launch_rejected: App bundle is unavailable."))?;
    authorize_launch(source, &paths, &current_app)
        .map_err(|_| String::from("desktop_launch_rejected: Launch is not authorized."))?;

    let setup_paths = paths.clone();
    let setup_app = current_app.clone();
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(
            |app, arguments, _working_directory| {
                let source = parse_process_arguments(arguments.into_iter().map(OsString::from));
                if matches!(source, Ok(LaunchSource::UserInteractive)) {
                    composition::restore_main_window(app);
                }
            },
        ))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--background", "--launch-source=login"]),
        ))
        .on_window_event(|window, event| {
            if window.label() == "main"
                && let tauri::WindowEvent::CloseRequested { api, .. } = event
            {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(move |app| composition::setup(app, &setup_paths, source, &setup_app))
        .invoke_handler(composition::invoke_handler())
        .build(tauri::generate_context!())
        .map_err(|_| String::from("desktop_start_failed: Desktop Host could not be built."))?;
    app.run(|app, event| match event {
        #[cfg(target_os = "macos")]
        tauri::RunEvent::Reopen { .. } => composition::restore_main_window(app),
        tauri::RunEvent::ExitRequested {
            code: None, api, ..
        } => {
            if let Some(state) = app.try_state::<composition::AppState>()
                && !state.system_shutdown_requested()
            {
                api.prevent_exit();
                if state.persist_user_quit_and_stop().is_ok() {
                    app.exit(0);
                }
            }
        }
        tauri::RunEvent::Exit => {
            if let Some(state) = app.try_state::<composition::AppState>()
                && !state.system_shutdown_requested()
            {
                let _ = state.persist_user_quit_and_stop();
            }
        }
        _ => {}
    });
    Ok(())
}
