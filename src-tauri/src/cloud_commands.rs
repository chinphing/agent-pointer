//! 账户信息命令。云主机商城已下线，这里只剩余额/额度查询，
//! 供余额条（`usePlatformBalance`）使用。

use pointer_core::chat_service::AppState;
use pointer_core::platform_agents::{fetch_platform_me, PlatformMe};
use std::sync::Arc;
use tauri::State;

async fn ensure_platform(
    auth: &pointer_core::platform_auth::PlatformAuthManager,
) -> Result<(), String> {
    crate::platform_auth_gate::require_logged_in(auth, "请先登录 Pointer 平台账户").await
}

#[tauri::command]
pub async fn get_cloud_platform_me(state: State<'_, Arc<AppState>>) -> Result<PlatformMe, String> {
    ensure_platform(state.platform_auth.as_ref()).await?;
    match fetch_platform_me(state.platform_auth.as_ref()).await {
        Ok(me) => Ok(me),
        Err(e) => {
            // 余额条取不到值时只显示「--」，界面层不会报错；这里留痕便于排查。
            log::warn!("get_cloud_platform_me failed: {e:#}");
            Err(e.to_string())
        }
    }
}
