use pointer_core::platform_auth::{run_platform_login_flow, PlatformSessionView};
use pointer_core::token_usage_queue;
use pointer_core::chat_service::AppState;
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub fn get_platform_session(state: State<'_, Arc<AppState>>) -> PlatformSessionView {
    state.platform_auth.session_view()
}

#[tauri::command]
pub async fn open_platform_login(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let auth = state.platform_auth.clone();
    run_platform_login_flow(auth)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn refresh_platform_session(state: State<'_, Arc<AppState>>) -> Result<PlatformSessionView, String> {
    state
        .platform_auth
        .refresh_if_needed()
        .await
        .map_err(|e| e.to_string())?;
    Ok(state.platform_auth.session_view())
}

#[tauri::command]
pub async fn logout_platform(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.platform_auth.clear_session();
    Ok(())
}

#[tauri::command]
pub async fn flush_platform_token_usage(state: State<'_, Arc<AppState>>) -> Result<u32, String> {
    let n = token_usage_queue::flush_pending_reports(&state.platform_auth)
        .await
        .map_err(|e| e.to_string())?;
    Ok(n as u32)
}

#[tauri::command]
pub async fn load_platform_session_from_keyring(state: State<'_, Arc<AppState>>) -> Result<bool, String> {
    state
        .platform_auth
        .load_from_keyring()
        .await
        .map_err(|e| e.to_string())
}
