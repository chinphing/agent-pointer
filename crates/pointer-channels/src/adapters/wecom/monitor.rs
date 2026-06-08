use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::ws_client::{run_wecom_ws_loop, WeComWsConfig};
use super::ws_state::{register_session, unregister_session, WeComWsSession};
use crate::gateway::ChannelGateway;

pub async fn run_wecom_monitor(
    gateway: Arc<ChannelGateway>,
    account_id: String,
    cancel: CancellationToken,
) -> Result<()> {
    let account = {
        let cfg = gateway.config();
        match cfg.wecom.get(&account_id) {
            Some(a) if a.enabled => a.clone(),
            _ => {
                log::warn!("wecom monitor skipped: account disabled or missing id={account_id}");
                return Ok(());
            }
        }
    };

    if account.connection_mode != "websocket" {
        log::info!("wecom monitor skipped: connection_mode={} id={account_id}", account.connection_mode);
        return Ok(());
    }
    if account.bot_id.trim().is_empty() || account.secret.trim().is_empty() {
        log::warn!("wecom ws monitor skipped: missing botId/secret account={account_id}");
        return Ok(());
    }

    let (outbound_tx, outbound_rx) = mpsc::unbounded_channel();
    let (inbound_tx, mut inbound_rx) = mpsc::unbounded_channel::<(Value, String)>();

    register_session(WeComWsSession::new(account_id.clone(), outbound_tx));

    let ws_cfg = WeComWsConfig {
        account_id: account_id.clone(),
        bot_id: account.bot_id.clone(),
        secret: account.secret.clone(),
        ws_url: account.websocket_url.clone(),
    };

    let ws_cancel = cancel.clone();
    let ws_result = run_wecom_ws_loop(
        ws_cfg,
        outbound_rx,
        inbound_tx,
        inbound_rx,
        gateway,
        ws_cancel,
    )
    .await;
    unregister_session(&account_id);

    if let Err(e) = ws_result {
        log::error!("wecom ws monitor stopped account={account_id}: {e:#}");
    } else {
        log::info!("wecom ws monitor stopped account={account_id}");
    }
    Ok(())
}
