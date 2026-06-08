use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use pointer_channels::{
    config::ChannelsConfig,
    traits::WebhookContext,
    webhook::guards::WebhookGuards,
    webhook::handler::WebhookQuery,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::ServerState;

static WEBHOOK_GUARDS: std::sync::OnceLock<WebhookGuards> = std::sync::OnceLock::new();

fn guards() -> &'static WebhookGuards {
    WEBHOOK_GUARDS.get_or_init(WebhookGuards::new)
}

pub async fn channel_webhook(
    State(state): State<ServerState>,
    Path((channel, account_id)): Path<(String, String)>,
    Query(query): Query<WebhookQuery>,
    method: axum::http::Method,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let rate_key = format!("{channel}:{account_id}");
    if !guards().check_rate(&rate_key) {
        return (StatusCode::TOO_MANY_REQUESTS, "rate limited").into_response();
    }
    if !WebhookGuards::check_body_size(body.len()) {
        return (StatusCode::PAYLOAD_TOO_LARGE, "body too large").into_response();
    }

    let gateway = state.channel_gateway.clone();
    let Some(account) = gateway.config().account(&channel, &account_id).cloned() else {
        return (StatusCode::NOT_FOUND, "unknown account").into_response();
    };
    if !account.enabled {
        return (StatusCode::SERVICE_UNAVAILABLE, "account disabled").into_response();
    }

    let plugin = match gateway.registry().get(&channel) {
        Some(p) => p,
        None => return (StatusCode::NOT_FOUND, "unknown channel").into_response(),
    };

    let query_str = build_query_string(&query);
    let ctx = WebhookContext {
        channel: plugin.channel_id(),
        account_id: &account_id,
        account: &account,
        headers: &headers,
        raw_body: &body,
        method: method.as_str(),
        query: &query_str,
    };

    let inbound = parse_inbound_payload(&channel, &body, &account_id, &plugin);

    let response = match plugin.webhook.handle_webhook(ctx).await {
        Ok(resp) => {
            let mut builder = axum::response::Response::builder().status(resp.status);
            if !resp.content_type.is_empty() {
                builder = builder.header(axum::http::header::CONTENT_TYPE, resp.content_type);
            }
            builder
                .body(axum::body::Body::from(resp.body))
                .unwrap()
                .into_response()
        }
        Err(e) => {
            log::error!("webhook handler error channel={channel} account={account_id}: {e:#}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "handler error").into_response();
        }
    };

    if let Some(msg) = inbound {
        tokio::spawn(async move {
            if let Err(e) = gateway.process_inbound(msg).await {
                log::error!("channel process_inbound failed: {e:#}");
            }
        });
    }
    response
}

fn build_query_string(q: &WebhookQuery) -> String {
    let mut parts = Vec::new();
    if !q.msg_signature.is_empty() {
        parts.push(format!("msg_signature={}", q.msg_signature));
    }
    if !q.timestamp.is_empty() {
        parts.push(format!("timestamp={}", q.timestamp));
    }
    if !q.nonce.is_empty() {
        parts.push(format!("nonce={}", q.nonce));
    }
    if !q.echostr.is_empty() {
        parts.push(format!("echostr={}", q.echostr));
    }
    parts.join("&")
}

fn parse_inbound_payload(
    channel: &str,
    body: &Bytes,
    account_id: &str,
    plugin: &Arc<pointer_channels::traits::ChannelPlugin>,
) -> Option<pointer_channels::traits::InboundMessage> {
    if channel == "wecom" {
        let raw = std::str::from_utf8(body).ok()?;
        if raw.contains("<Encrypt>") {
            let enc = extract_xml_tag(raw, "Encrypt")?;
            let event = serde_json::json!({ "Encrypt": enc });
            return plugin.webhook.parse_inbound(&event, account_id);
        }
    }
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|v| plugin.webhook.parse_inbound(&v, account_id))
}

fn extract_xml_tag(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = xml.find(&close)?;
    let inner = &xml[start..end];
    if inner.starts_with("<![CDATA[") && inner.ends_with("]]>") {
        Some(inner[9..inner.len() - 3].to_string())
    } else {
        Some(inner.to_string())
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelsStatusResponse {
    channels: Vec<ChannelStatusItem>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelStatusItem {
    channel: String,
    account_id: String,
    enabled: bool,
    webhook_url: String,
}

pub async fn get_channels_config(State(state): State<ServerState>) -> Json<ChannelsConfig> {
    Json(state.channel_gateway.config().clone())
}

pub async fn list_channels(State(state): State<ServerState>) -> Json<ChannelsStatusResponse> {
    let cfg = state.channel_gateway.config();
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
                webhook_url: cfg.webhook_url(channel, account_id),
            });
        }
    }
    Json(ChannelsStatusResponse { channels })
}

pub async fn update_channels(
    State(state): State<ServerState>,
    Json(cfg): Json<ChannelsConfig>,
) -> Result<StatusCode, StatusCode> {
    state
        .channel_gateway
        .update_config(cfg)
        .map_err(|e| {
            log::error!("update channels config failed: {e:#}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn get_channel_webhook_url(
    State(state): State<ServerState>,
    Path((channel, account_id)): Path<(String, String)>,
) -> Json<serde_json::Value> {
    let cfg = state.channel_gateway.config();
    Json(serde_json::json!({
        "webhookUrl": cfg.webhook_url(&channel, &account_id)
    }))
}

pub async fn start_weixin_login(
    State(state): State<ServerState>,
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

pub async fn weixin_login_status(
    State(state): State<ServerState>,
    Path(account_id): Path<String>,
) -> Json<serde_json::Value> {
    let session = state.qr_login.get(&account_id).await;
    Json(serde_json::json!(session))
}

#[derive(Deserialize)]
pub struct ApprovePayload {
    code: String,
}

pub async fn approve_channel_pairing(
    State(state): State<ServerState>,
    Path((channel, account_id)): Path<(String, String)>,
    Json(payload): Json<ApprovePayload>,
) -> Result<StatusCode, StatusCode> {
    let ok = state
        .channel_gateway
        .pairing
        .approve(&channel, &account_id, &payload.code)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if ok {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

pub async fn list_channel_pairing_pending(
    State(state): State<ServerState>,
    Path((channel, account_id)): Path<(String, String)>,
) -> Json<serde_json::Value> {
    let pending = state
        .channel_gateway
        .pairing
        .list_pending(&channel, &account_id);
    Json(serde_json::json!({ "pending": pending }))
}
