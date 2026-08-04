#[cfg(target_os = "macos")]
mod macos_workspace;

#[cfg(target_os = "macos")]
pub use macos_workspace::install_workspace_observers;

#[cfg(not(target_os = "macos"))]
pub fn install_workspace_observers(_app: &tauri::AppHandle) -> Result<(), String> {
    Ok(())
}
