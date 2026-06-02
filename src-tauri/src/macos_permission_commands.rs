//! Tauri commands for macOS Computer permissions (desktop only).

use crate::macos_computer_permissions::{self, MacosComputerPermissionsStatus};
use tauri::AppHandle;

#[tauri::command]
pub fn get_macos_computer_permissions(
    app: AppHandle,
) -> Result<MacosComputerPermissionsStatus, String> {
    macos_computer_permissions::status_on_main(&app)
}

/// `CGRequestScreenCaptureAccess` — register current process in Screen Recording list.
#[tauri::command]
pub fn register_macos_screen_recording_access() {
    let registered =
        pointer_core::platform::macos_permissions::register_screen_recording_in_settings();
    log::info!("macOS CGRequestScreenCaptureAccess (permission wizard): {registered}");
}

#[tauri::command]
pub fn open_macos_computer_permission_settings(kind: String) -> Result<(), String> {
    macos_computer_permissions::open_settings(&kind)
}

#[tauri::command]
pub fn begin_macos_permission_drag_flow(app: AppHandle, kind: String) -> Result<(), String> {
    macos_computer_permissions::begin_drag_grant_flow(&app, &kind)
}

#[tauri::command]
pub fn dismiss_macos_permission_drag_guide(app: AppHandle) -> Result<(), String> {
    macos_computer_permissions::dismiss_drag_guide(&app)
}
