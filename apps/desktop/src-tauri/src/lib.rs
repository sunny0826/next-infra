//! Desktop Host modules composed by the `next-infra` binary.

pub mod adapter;
pub mod composition;
pub mod host;
pub mod keychain;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(
            |app, _arguments, _working_directory| {
                if let Some(window) = tauri::Manager::get_webview_window(app, "main") {
                    let _ = window.show();
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                }
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
        .run(tauri::generate_context!())
        .expect("failed to run Next Infra desktop host");
}
