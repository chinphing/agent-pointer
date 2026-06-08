use anyhow::Result;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use super::ws_client::{run_feishu_ws_loop, FeishuWsConfig};
use crate::gateway::ChannelGateway;

pub async fn run_feishu_monitor(
    gateway: Arc<ChannelGateway>,
    account_id: String,
    cancel: CancellationToken,
) -> Result<()> {
    let account = {
        let cfg = gateway.config();
        match cfg.feishu.get(&account_id) {
            Some(a) if a.enabled => a.clone(),
            _ => {
                log::warn!("feishu monitor skipped: account disabled or missing id={account_id}");
                return Ok(());
            }
        }
    };

    if account.connection_mode != "websocket" {
        log::info!(
            "feishu monitor skipped: connection_mode={} id={account_id}",
            account.connection_mode
        );
        return Ok(());
    }
    if account.app_id.trim().is_empty() || account.app_secret.trim().is_empty() {
        log::warn!("feishu ws monitor skipped: missing appId/appSecret account={account_id}");
        return Ok(());
    }

    let ws_cfg = FeishuWsConfig {
        account_id: account_id.clone(),
        app_id: account.app_id.clone(),
        app_secret: account.app_secret.clone(),
    };

    if let Err(e) = run_feishu_ws_loop(ws_cfg, gateway, cancel).await {
        log::error!("feishu ws monitor stopped account={account_id}: {e:#}");
    } else {
        log::info!("feishu ws monitor stopped account={account_id}");
    }
    Ok(())
}
