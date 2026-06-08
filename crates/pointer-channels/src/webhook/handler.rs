use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use std::sync::Arc;

use crate::gateway::ChannelGateway;
use crate::traits::WebhookContext;
use crate::webhook::guards::WebhookGuards;

pub struct WebhookState {
    pub gateway: Arc<ChannelGateway>,
    pub guards: WebhookGuards,
}

#[derive(serde::Deserialize, Default)]
pub struct WebhookQuery {
    #[serde(default)]
    pub msg_signature: String,
    #[serde(default)]
    pub timestamp: String,
    #[serde(default)]
    pub nonce: String,
    #[serde(default)]
    pub echostr: String,
}

pub async fn handle_channel_webhook(
    State(state): State<Arc<WebhookState>>,
    Path((channel, account_id)): Path<(String, String)>,
    Query(query): Query<WebhookQuery>,
    method: axum::http::Method,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let rate_key = format!("{channel}:{account_id}");
    if !state.guards.check_rate(&rate_key) {
        log::warn!("webhook rate limited channel={channel} account={account_id}");
        return (StatusCode::TOO_MANY_REQUESTS, "rate limited").into_response();
    }
    if !WebhookGuards::check_body_size(body.len()) {
        log::warn!("webhook body too large channel={channel} account={account_id}");
        return (StatusCode::PAYLOAD_TOO_LARGE, "body too large").into_response();
    }

    let Some(account) = state.gateway.config().account(&channel, &account_id).cloned() else {
        log::warn!("webhook unknown account channel={channel} account={account_id}");
        return (StatusCode::NOT_FOUND, "unknown account").into_response();
    };
    if !account.enabled {
        return (StatusCode::SERVICE_UNAVAILABLE, "account disabled").into_response();
    }

    let plugin = match state.gateway.registry().get(&channel) {
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
        let gateway = state.gateway.clone();
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
    plugin: &std::sync::Arc<crate::traits::ChannelPlugin>,
) -> Option<crate::traits::InboundMessage> {
    if channel == "wecom" {
        let raw = std::str::from_utf8(body).ok()?;
        if raw.contains("<xml>") || raw.contains("<Encrypt>") {
            let event = if raw.contains("<Encrypt>") {
                let enc = extract_xml_tag(raw, "Encrypt")?;
                serde_json::json!({ "Encrypt": enc })
            } else {
                return plugin.webhook.parse_inbound(
                    &serde_json::json!({ "xml": raw }),
                    account_id,
                );
            };
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
