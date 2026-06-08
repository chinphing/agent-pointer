use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use super::ilink_client::{ILinkClient, WeixinCredentials};
use crate::credentials::load_encrypted_json;
use crate::gateway::ChannelGateway;
use crate::session::build_conversation_key;
use crate::traits::{InboundMessage, InboundReplyContext};

pub async fn run_weixin_monitor(
    gateway: Arc<ChannelGateway>,
    account_id: String,
    cancel: CancellationToken,
) -> Result<()> {
    let creds: WeixinCredentials = match load_encrypted_json("weixin", &account_id)? {
        Some(c) => c,
        None => {
            log::warn!("weixin monitor skipped: no credentials account={account_id}");
            return Ok(());
        }
    };
    let client = ILinkClient::new(account_id.clone(), creds);
    let mut context_tokens: HashMap<String, String> = HashMap::new();
    log::info!("weixin monitor started account={account_id}");

    loop {
        if cancel.is_cancelled() {
            break;
        }
        let updates = match client.get_updates(35).await {
            Ok(v) => v,
            Err(e) => {
                log::warn!("weixin getupdates error account={account_id}: {e:#}");
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                continue;
            }
        };
        let msgs = updates
            .get("msgs")
            .or_else(|| updates.get("messages"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        if !msgs.is_empty() {
            log::info!(
                "weixin getupdates received {} message(s) account={account_id}",
                msgs.len()
            );
        }
        for msg in msgs {
            match parse_weixin_message(&msg, &account_id, &mut context_tokens) {
                Some(inbound) => {
                    log::info!(
                        "weixin inbound sender={} text_len={}",
                        inbound.sender_id,
                        inbound.text.chars().count()
                    );
                    if let Err(e) = gateway.process_inbound(inbound).await {
                        log::error!("weixin process_inbound failed: {e:#}");
                    }
                }
                None => {
                    log::debug!("weixin message skipped (non-user or unsupported): {msg}");
                }
            }
        }
    }
    log::info!("weixin monitor stopped account={account_id}");
    Ok(())
}

pub fn parse_weixin_message(
    msg: &serde_json::Value,
    account_id: &str,
    tokens: &mut HashMap<String, String>,
) -> Option<InboundMessage> {
    let msg_type = msg.get("message_type").and_then(json_as_u64);
    if let Some(t) = msg_type {
        if t != 1 {
            return None;
        }
    }
    let from = msg
        .get("from_user_id")
        .or_else(|| msg.get("from_user"))
        .or_else(|| msg.get("from"))
        .and_then(|v| v.as_str())?;
    if let Some(token) = msg.get("context_token").and_then(|v| v.as_str()) {
        tokens.insert(from.to_string(), token.to_string());
    }
    let message_id = msg
        .get("message_id")
        .or_else(|| msg.get("msg_id"))
        .or_else(|| msg.get("id"))
        .map(json_value_to_id)
        .unwrap_or_else(|| "unknown".into());
    let items = msg
        .get("item_list")
        .or_else(|| msg.get("items"))
        .and_then(|v| v.as_array())?;
    let text = items.iter().find_map(extract_text_item)?;
    let context_token = msg
        .get("context_token")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| tokens.get(from).cloned());
    Some(InboundMessage {
        channel: "weixin".into(),
        account_id: account_id.into(),
        message_id,
        conversation_key: build_conversation_key("weixin", from, false),
        sender_id: from.to_string(),
        sender_name: None,
        text,
        is_group: false,
        mentioned_bot: true,
        reply_context: Some(InboundReplyContext {
            session_webhook: None,
            chat_id: None,
            open_id: Some(from.to_string()),
            context_token,
            wecom_req_id: None,
        }),
    })
}

fn json_as_u64(v: &serde_json::Value) -> Option<u64> {
    v.as_u64()
        .or_else(|| v.as_i64().and_then(|n| u64::try_from(n).ok()))
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

fn json_value_to_id(v: &serde_json::Value) -> String {
    if let Some(s) = v.as_str() {
        s.to_string()
    } else if let Some(n) = v.as_i64() {
        n.to_string()
    } else if let Some(n) = v.as_u64() {
        n.to_string()
    } else {
        v.to_string()
    }
}

fn extract_text_item(it: &serde_json::Value) -> Option<String> {
    let ty = it.get("type")?;
    let is_text = ty.as_u64() == Some(1)
        || ty.as_i64() == Some(1)
        || ty.as_str().is_some_and(|s| s == "text" || s == "1");
    if !is_text {
        return None;
    }
    it.get("text_item")
        .and_then(|t| t.get("text"))
        .or_else(|| it.get("text"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_weixin_message_reads_ilink_format() {
        let mut tokens = HashMap::new();
        let inbound = parse_weixin_message(
            &json!({
                "message_id": 9812451782375u64,
                "from_user_id": "user@im.wechat",
                "message_type": 1,
                "context_token": "token-abc",
                "item_list": [{ "type": 1, "text_item": { "text": "你好" } }]
            }),
            "default",
            &mut tokens,
        )
        .expect("parse");
        assert_eq!(inbound.sender_id, "user@im.wechat");
        assert_eq!(inbound.text, "你好");
        assert_eq!(
            inbound.reply_context.as_ref().unwrap().context_token.as_deref(),
            Some("token-abc")
        );
    }
}
