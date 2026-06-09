use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use super::ilink_client::{ILinkClient, WeixinCredentials};
use super::parse::parse_weixin_message;
use crate::connection_state;
use crate::credentials::load_encrypted_json;
use crate::gateway::ChannelGateway;

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
    let _connected = connection_state::ConnectionGuard::connect("weixin", &account_id);

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
                        "weixin inbound sender={} text_len={} attachments={}",
                        inbound.sender_id,
                        inbound.text.chars().count(),
                        inbound.attachments.len()
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
