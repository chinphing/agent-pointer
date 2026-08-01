//! Tauri commands for cloud agent management and remote WebView.

use pointer_core::chat_service::AppState;
use pointer_core::platform_agents::{
    create_agent_oauth_code, fetch_platform_me, fetch_shop_pricing, get_agent, list_agents,
    list_shop_regions, preview_shop, purchase_agent, release_agent, renew_agent, AgentListPage,
    AgentOauthCodeResult, CloudAgent, PlatformMe, ShopPreviewRequest, ShopPreviewResult,
    ShopPricing, ShopPurchaseResult, ShopRegion,
};
use std::sync::Arc;
use tauri::{AppHandle, State};

use crate::cloud_webview;

async fn ensure_platform(
    auth: &pointer_core::platform_auth::PlatformAuthManager,
) -> Result<(), String> {
    crate::platform_auth_gate::require_logged_in(auth, "请先登录 Pointer 平台账户").await
}

#[tauri::command]
pub async fn get_cloud_platform_me(state: State<'_, Arc<AppState>>) -> Result<PlatformMe, String> {
    ensure_platform(state.platform_auth.as_ref()).await?;
    fetch_platform_me(state.platform_auth.as_ref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_cloud_shop_regions(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<ShopRegion>, String> {
    ensure_platform(state.platform_auth.as_ref()).await?;
    list_shop_regions(state.platform_auth.as_ref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_cloud_shop_pricing(
    state: State<'_, Arc<AppState>>,
    region_id: String,
) -> Result<ShopPricing, String> {
    ensure_platform(state.platform_auth.as_ref()).await?;
    fetch_shop_pricing(state.platform_auth.as_ref(), &region_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn preview_cloud_shop(
    state: State<'_, Arc<AppState>>,
    body: ShopPreviewRequest,
) -> Result<ShopPreviewResult, String> {
    ensure_platform(state.platform_auth.as_ref()).await?;
    preview_shop(state.platform_auth.as_ref(), &body)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn purchase_cloud_agent(
    state: State<'_, Arc<AppState>>,
    body: ShopPreviewRequest,
) -> Result<ShopPurchaseResult, String> {
    ensure_platform(state.platform_auth.as_ref()).await?;
    purchase_agent(state.platform_auth.as_ref(), &body)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_cloud_agents(
    state: State<'_, Arc<AppState>>,
    page: i32,
    page_size: i32,
    status: String,
) -> Result<AgentListPage, String> {
    ensure_platform(state.platform_auth.as_ref()).await?;
    list_agents(
        state.platform_auth.as_ref(),
        page.max(1),
        page_size.clamp(1, 100),
        &status,
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_cloud_agent(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
) -> Result<CloudAgent, String> {
    ensure_platform(state.platform_auth.as_ref()).await?;
    get_agent(state.platform_auth.as_ref(), &agent_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn renew_cloud_agent(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
    body: ShopPreviewRequest,
) -> Result<ShopPurchaseResult, String> {
    ensure_platform(state.platform_auth.as_ref()).await?;
    renew_agent(state.platform_auth.as_ref(), &agent_id, &body)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn release_cloud_agent(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    agent_id: String,
) -> Result<CloudAgent, String> {
    ensure_platform(state.platform_auth.as_ref()).await?;
    let agent = release_agent(state.platform_auth.as_ref(), &agent_id)
        .await
        .map_err(|e| e.to_string())?;
    let _ = cloud_webview::close_cloud_agent_window(&app, &agent_id);
    Ok(agent)
}

#[tauri::command]
pub async fn create_cloud_agent_oauth_code(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
) -> Result<AgentOauthCodeResult, String> {
    ensure_platform(state.platform_auth.as_ref()).await?;
    if let Err(e) = state.platform_auth.ensure_llm_allowed().await {
        return Err(e.to_string());
    }
    create_agent_oauth_code(state.platform_auth.as_ref(), &agent_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn open_cloud_agent(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    agent_id: String,
) -> Result<(), String> {
    ensure_platform(state.platform_auth.as_ref()).await?;
    cloud_webview::open_cloud_agent_window(app, state.inner().clone(), agent_id).await
}

#[tauri::command]
pub fn focus_cloud_agent(app: AppHandle, agent_id: String) -> Result<(), String> {
    cloud_webview::focus_cloud_agent_window(&app, &agent_id)
}

#[tauri::command]
pub fn close_cloud_agent(app: AppHandle, agent_id: String) -> Result<(), String> {
    cloud_webview::close_cloud_agent_window(&app, &agent_id)
}

#[tauri::command]
pub fn is_cloud_agent_window_open(app: AppHandle, agent_id: String) -> bool {
    cloud_webview::is_cloud_agent_window_open(&app, &agent_id)
}
