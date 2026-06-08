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
        for msg in msgs {
            if let Some(inbound) = parse_weixin_message(&msg, &account_id, &mut context_tokens) {
                if let Err(e) = gateway.process_inbound(inbound).await {
                    log::error!("weixin process_inbound failed: {e:#}");
                }
            }
        }
    }
    log::info!("weixin monitor stopped account={account_id}");
    Ok(())
}

fn parse_weixin_message(
    msg: &serde_json::Value,
    account_id: &str,
    tokens: &mut HashMap<String, String>,
) -> Option<InboundMessage> {
    let from = msg.get("from_user").or_else(|| msg.get("from"))?.as_str()?;
    if let Some(token) = msg.get("context_token").and_then(|v| v.as_str()) {
        tokens.insert(from.to_string(), token.to_string());
    }
    let message_id = msg
        .get("msg_id")
        .or_else(|| msg.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let items = msg.get("items").and_then(|v| v.as_array())?;
    let text = items
        .iter()
        .find_map(|it| {
            if it.get("type").and_then(|v| v.as_str()) == Some("text") {
                it.get("text").and_then(|v| v.as_str()).map(|s| s.to_string())
            } else {
                None
            }
        })?;
    let context_token = tokens.get(from).cloned();
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
