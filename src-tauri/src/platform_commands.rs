use pointer_core::chat_service::AppState;
use pointer_core::platform_auth::{run_platform_login_flow, PlatformSessionView};
use pointer_core::platform_config::apply_login_media_oss;
use pointer_core::token_usage_store;
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub fn get_platform_session(state: State<'_, Arc<AppState>>) -> PlatformSessionView {
    state.platform_auth.session_view()
}

/// Mirror of the web server's `GET /api/auth/mode`: `standalone` when no control
/// plane is bound, `platform` otherwise.
#[tauri::command]
pub fn get_auth_mode() -> String {
    if pointer_core::deployment_mode::is_standalone() {
        "standalone".to_string()
    } else {
        "platform".to_string()
    }
}

#[tauri::command]
pub async fn open_platform_login(
    state: State<'_, Arc<AppState>>,
) -> Result<PlatformSessionView, String> {
    let auth = state.platform_auth.clone();
    let app = state.inner().clone();
    let (_session, creds) = run_platform_login_flow(auth)
        .await
        .map_err(|e| e.to_string())?;
    app.apply_login_credentials(&creds);
    Ok(app.platform_auth.session_view())
}

#[tauri::command]
pub fn cancel_platform_login(state: State<'_, Arc<AppState>>) {
    state.platform_auth.cancel_pending_login();
}

#[tauri::command]
pub async fn refresh_platform_session(
    state: State<'_, Arc<AppState>>,
) -> Result<PlatformSessionView, String> {
    state
        .platform_auth
        .refresh_if_needed()
        .await
        .map_err(|e| e.to_string())?;
    // Fetch LLM creds with a short bound so the UI is not stuck on "等待授权"
    // after oauth exchange already succeeded.
    if state.platform_auth.session_view().logged_in {
        match tokio::time::timeout(
            std::time::Duration::from_secs(8),
            state.platform_auth.fetch_llm_credentials(),
        )
        .await
        {
            Ok(Ok(Some(creds))) => state.apply_login_credentials(&creds),
            Ok(Ok(None)) => {}
            Ok(Err(e)) => {
                log::warn!("platform_auth: refresh llm-credentials failed: {e:#}");
            }
            Err(_) => {
                log::warn!("platform_auth: refresh llm-credentials timed out");
            }
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
    apply_login_media_oss(&mut platform.media_oss, None);
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
pub fn list_token_usage(
    from: Option<String>,
    to: Option<String>,
) -> Result<token_usage_store::TokenUsageListResult, String> {
    token_usage_store::list_token_usage(from.as_deref(), to.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn load_platform_session_persisted(
    state: State<'_, Arc<AppState>>,
) -> Result<bool, String> {
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
pub async fn load_platform_session_from_keyring(
    state: State<'_, Arc<AppState>>,
) -> Result<bool, String> {
    load_platform_session_persisted(state).await
}
