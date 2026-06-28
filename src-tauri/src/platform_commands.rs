use pointer_core::platform_auth::{run_platform_login_flow, PlatformSessionView};
use pointer_core::platform_config::apply_login_media_oss;
use pointer_core::token_usage_store;
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
    let app = state.inner().clone();
    let (_session, creds) = run_platform_login_flow(auth)
        .await
        .map_err(|e| e.to_string())?;
    app.apply_login_credentials(&creds);
    Ok(())
}

#[tauri::command]
pub fn cancel_platform_login(state: State<'_, Arc<AppState>>) {
    state.platform_auth.cancel_pending_login();
}

#[tauri::command]
pub async fn refresh_platform_session(state: State<'_, Arc<AppState>>) -> Result<PlatformSessionView, String> {
    state
        .platform_auth
        .refresh_if_needed()
        .await
        .map_err(|e| e.to_string())?;
    if state.platform_auth.session_view().logged_in {
        if let Ok(Some(creds)) = state.platform_auth.fetch_llm_credentials().await {
            state.apply_login_credentials(&creds);
        }
    }
    Ok(state.platform_auth.session_view())
}

#[tauri::command]
pub async fn logout_platform(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    log::info!("platform_auth: logout_platform command start");
    state.platform_auth.clear_session_async().await;
    log::info!("platform_auth: clear_session_async done");
    let mut platform = state.platform_config.write();
    apply_login_media_oss(&mut platform, None);
    log::info!("platform_auth: logout_platform command done");
    Ok(())
}

#[tauri::command]
pub async fn flush_platform_token_usage(state: State<'_, Arc<AppState>>) -> Result<u32, String> {
    let n = token_usage_store::flush_pending_reports(&state.platform_auth)
        .await
        .map_err(|e| e.to_string())?;
    Ok(n as u32)
}

#[tauri::command]
pub async fn load_platform_session_persisted(state: State<'_, Arc<AppState>>) -> Result<bool, String> {
    let creds = state
        .platform_auth
        .load_persisted_session()
        .await
        .map_err(|e| e.to_string())?;
    if let Some(c) = creds {
        state.apply_login_credentials(&c);
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Back-compat alias.
#[tauri::command]
pub async fn load_platform_session_from_keyring(state: State<'_, Arc<AppState>>) -> Result<bool, String> {
    load_platform_session_persisted(state).await
}
