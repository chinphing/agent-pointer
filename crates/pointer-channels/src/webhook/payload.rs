use axum::body::Bytes;
use serde_json::Value;
use std::sync::Arc;

use crate::config::ChannelAccountConfig;
use crate::crypto::feishu_decode_event_body;
use crate::traits::{ChannelPlugin, InboundMessage};

pub fn decode_webhook_event(
    channel: &str,
    body: &Bytes,
    account: &ChannelAccountConfig,
) -> Option<Value> {
    let raw = std::str::from_utf8(body).ok()?;
    if channel == "feishu" {
        return feishu_decode_event_body(raw, &account.encrypt_key)
            .map_err(|e| {
                log::warn!("feishu webhook decode failed: {e:#}");
                e
            })
            .ok();
    }
    serde_json::from_slice(body).ok()
}

pub fn parse_inbound_payload(
    channel: &str,
    body: &Bytes,
    account_id: &str,
    account: &ChannelAccountConfig,
    plugin: &Arc<ChannelPlugin>,
) -> Option<InboundMessage> {
    if channel == "wecom" {
        let raw = std::str::from_utf8(body).ok()?;
        if raw.contains("<xml>") || raw.contains("<Encrypt>") {
            let event = if raw.contains("<Encrypt>") {
                let enc = extract_xml_tag(raw, "Encrypt")?;
                Value::from(serde_json::json!({ "Encrypt": enc }))
            } else {
                return plugin.webhook.parse_inbound(
                    &serde_json::json!({ "xml": raw }),
                    account_id,
                );
            };
            return plugin.webhook.parse_inbound(&event, account_id);
        }
    }

    let event = decode_webhook_event(channel, body, account)?;
    plugin.webhook.parse_inbound(&event, account_id)
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
