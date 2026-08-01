//! Shared login gate for Tauri commands that need a fresh platform session.

use pointer_core::chat_service::AppState;
use pointer_core::platform_auth::PlatformAuthManager;
use std::sync::Arc;

/// Refresh access if needed, then require `logged_in`.
pub async fn require_logged_in(
    auth: &PlatformAuthManager,
    login_hint: &str,
) -> Result<(), String> {
    auth.refresh_if_needed()
        .await
        .map_err(|e| e.to_string())?;
    if !auth.session_view().logged_in {
        return Err(login_hint.into());
    }
    Ok(())
}

/// Desktop attachment / session-scoped paths: refresh, logged_in, and platform user id.
pub async fn require_platform_user_id(state: &Arc<AppState>) -> Result<String, String> {
    const HINT: &str = "请先登录 Pointer 账户";
    let auth = state.active_platform_auth();
    require_logged_in(auth.as_ref(), HINT).await?;
    auth.platform_user_id()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| HINT.to_string())
}
