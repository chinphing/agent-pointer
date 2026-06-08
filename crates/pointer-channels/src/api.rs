use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::adapters::weixin::qr_login::QrLoginState;
use crate::config::{account_runtime_connected, ChannelsConfig};
use crate::gateway::ChannelGateway;
use crate::registration::ChannelRegistrationState;

#[derive(Clone)]
pub struct ChannelApiState {
    pub gateway: Arc<ChannelGateway>,
    pub qr_login: Arc<QrLoginState>,
    pub registration: Arc<ChannelRegistrationState>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChannelsStatusResponse {
    channels: Vec<ChannelStatusItem>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChannelStatusItem {
    channel: String,
    account_id: String,
    enabled: bool,
    connected: bool,
    webhook_url: String,
}

pub fn channel_api_routes(state: Arc<ChannelApiState>) -> Router {
    Router::new()
        .route("/api/channels", get(list_channels).put(update_channels))
        .route("/api/channels/:channel/:account_id/webhook-url", get(get_webhook_url))
        .route(
            "/api/channels/weixin/:account_id/login/start",
            post(start_weixin_login),
        )
        .route(
            "/api/channels/weixin/:account_id/login/status",
            get(weixin_login_status),
        )
        .route(
            "/api/channels/:channel/:account_id/register/start",
            post(start_channel_registration),
        )
        .route(
            "/api/channels/:channel/:account_id/register/status",
            get(channel_registration_status),
        )
        .route(
            "/api/channels/:channel/:account_id/pairing/approve",
            post(approve_pairing),
        )
        .route(
            "/api/channels/:channel/:account_id/pairing/pending",
            get(list_pending_pairing),
        )
        .with_state(state)
}

async fn list_channels(State(state): State<Arc<ChannelApiState>>) -> Json<ChannelsStatusResponse> {
    let cfg = state.gateway.config();
    let mut channels = Vec::new();
    for (channel, map) in [
        ("feishu", &cfg.feishu),
        ("dingtalk", &cfg.dingtalk),
        ("wecom", &cfg.wecom),
        ("weixin", &cfg.weixin),
    ] {
        for (account_id, account) in map {
            channels.push(ChannelStatusItem {
                channel: channel.into(),
                account_id: account_id.clone(),
                enabled: account.enabled,
                connected: account_runtime_connected(channel, account_id, account),
                webhook_url: cfg.webhook_url(channel, account_id),
            });
        }
    }
    Json(ChannelsStatusResponse { channels })
}

async fn update_channels(
    State(state): State<Arc<ChannelApiState>>,
    Json(cfg): Json<ChannelsConfig>,
) -> Result<StatusCode, StatusCode> {
    state
        .gateway
        .update_config(cfg)
        .map_err(|e| {
            log::error!("update channels config failed: {e:#}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(StatusCode::NO_CONTENT)
}

async fn get_webhook_url(
    State(state): State<Arc<ChannelApiState>>,
    Path((channel, account_id)): Path<(String, String)>,
) -> Json<serde_json::Value> {
    let cfg = state.gateway.config();
    Json(serde_json::json!({
        "webhookUrl": cfg.webhook_url(&channel, &account_id)
    }))
}

async fn start_weixin_login(
    State(state): State<Arc<ChannelApiState>>,
    Path(account_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let session = state
        .qr_login
        .start(&account_id)
        .await
        .map_err(|e| {
            log::error!("weixin login start failed: {e:#}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Json(serde_json::json!(session)))
}

async fn weixin_login_status(
    State(state): State<Arc<ChannelApiState>>,
    Path(account_id): Path<String>,
) -> Json<serde_json::Value> {
    let session = state.qr_login.get(&account_id).await;
    Json(serde_json::json!(session))
}

async fn start_channel_registration(
    State(state): State<Arc<ChannelApiState>>,
    Path((channel, account_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let session = state
        .registration
        .start(&channel, &account_id)
        .await
        .map_err(|e| {
            log::error!("channel registration start failed channel={channel}: {e:#}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Json(serde_json::json!(session)))
}

async fn channel_registration_status(
    State(state): State<Arc<ChannelApiState>>,
    Path((channel, account_id)): Path<(String, String)>,
) -> Json<serde_json::Value> {
    let session = state.registration.get(&channel, &account_id).await;
    Json(serde_json::json!(session))
}

#[derive(Deserialize)]
struct ApprovePayload {
    code: String,
}

async fn approve_pairing(
    State(state): State<Arc<ChannelApiState>>,
    Path((channel, account_id)): Path<(String, String)>,
    Json(payload): Json<ApprovePayload>,
) -> Result<StatusCode, StatusCode> {
    let ok = state
        .gateway
        .pairing
        .approve(&channel, &account_id, &payload.code)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if ok {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

async fn list_pending_pairing(
    State(state): State<Arc<ChannelApiState>>,
    Path((channel, account_id)): Path<(String, String)>,
) -> Json<serde_json::Value> {
    let pending = state.gateway.pairing.list_pending(&channel, &account_id);
    Json(serde_json::json!({ "pending": pending }))
}
