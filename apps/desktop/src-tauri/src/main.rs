fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(
            |_app, _arguments, _working_directory| {},
        ))
        .run(tauri::generate_context!())
        .expect("failed to run Next Infra desktop host");
}
