//! Desktop Host modules composed by the `next-infra` binary.

pub mod adapter;
pub mod composition;
pub mod host;
pub mod keychain;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(
            |app, _arguments, _working_directory| {
                composition::restore_main_window(app);
            },
        ))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .on_window_event(|window, event| {
            if window.label() == "main"
                && let tauri::WindowEvent::CloseRequested { api, .. } = event
            {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(composition::setup)
        .invoke_handler(composition::invoke_handler())
        .build(tauri::generate_context!())
        .expect("failed to build Next Infra desktop host")
        .run(|app, event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                composition::restore_main_window(app);
            }
        });
}
