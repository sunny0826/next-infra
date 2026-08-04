use crate::composition::AppState;
use block2::RcBlock;
use objc2_app_kit::{
    NSWorkspace, NSWorkspaceDidWakeNotification, NSWorkspaceWillPowerOffNotification,
    NSWorkspaceWillSleepNotification,
};
use objc2_foundation::NSNotification;
use std::ptr::NonNull;
use tauri::Manager;

pub fn install_workspace_observers(app: &tauri::AppHandle) -> Result<(), String> {
    let center = NSWorkspace::sharedWorkspace().notificationCenter();

    let sleep_app = app.clone();
    let sleep = RcBlock::new(move |_: NonNull<NSNotification>| {
        if let Some(state) = sleep_app.try_state::<AppState>() {
            state.handle_sleep();
        }
    });
    let wake_app = app.clone();
    let wake = RcBlock::new(move |_: NonNull<NSNotification>| {
        if let Some(state) = wake_app.try_state::<AppState>() {
            state.handle_wake();
        }
    });
    let power_app = app.clone();
    let power = RcBlock::new(move |_: NonNull<NSNotification>| {
        if let Some(state) = power_app.try_state::<AppState>() {
            state.handle_power_off();
        }
    });

    unsafe {
        center.addObserverForName_object_queue_usingBlock(
            Some(NSWorkspaceWillSleepNotification),
            None,
            None,
            &sleep,
        );
        center.addObserverForName_object_queue_usingBlock(
            Some(NSWorkspaceDidWakeNotification),
            None,
            None,
            &wake,
        );
        center.addObserverForName_object_queue_usingBlock(
            Some(NSWorkspaceWillPowerOffNotification),
            None,
            None,
            &power,
        );
    }
    Ok(())
}
