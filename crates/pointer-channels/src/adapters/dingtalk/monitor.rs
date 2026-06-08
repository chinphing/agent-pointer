use anyhow::Result;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use super::stream_client::{run_dingtalk_stream_loop, DingTalkStreamConfig};
use crate::gateway::ChannelGateway;

pub async fn run_dingtalk_monitor(
    gateway: Arc<ChannelGateway>,
    account_id: String,
    cancel: CancellationToken,
) -> Result<()> {
    let account = {
        let cfg = gateway.config();
        match cfg.dingtalk.get(&account_id) {
            Some(a) if a.enabled => a.clone(),
            _ => {
                log::warn!("dingtalk monitor skipped: account disabled or missing id={account_id}");
                return Ok(());
            }
        }
    };

    if account.connection_mode != "websocket" {
        log::info!(
            "dingtalk monitor skipped: connection_mode={} id={account_id}",
            account.connection_mode
        );
        return Ok(());
    }
    if account.client_id.trim().is_empty() || account.client_secret.trim().is_empty() {
        log::warn!("dingtalk stream monitor skipped: missing clientId/secret account={account_id}");
        return Ok(());
    }

    let stream_cfg = DingTalkStreamConfig {
        account_id: account_id.clone(),
        client_id: account.client_id.clone(),
        client_secret: account.client_secret.clone(),
    };

    if let Err(e) = run_dingtalk_stream_loop(stream_cfg, gateway, cancel).await {
        log::error!("dingtalk stream monitor stopped account={account_id}: {e:#}");
    } else {
        log::info!("dingtalk stream monitor stopped account={account_id}");
    }
    Ok(())
}
