//! Desktop Host modules composed by the `next-infra` binary.

pub mod adapter;
pub mod composition;
pub mod host;
pub mod keychain;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(
            |_app, _arguments, _working_directory| {},
        ))
        .setup(composition::setup)
        .invoke_handler(composition::invoke_handler())
        .run(tauri::generate_context!())
        .expect("failed to run Next Infra desktop host");
}
