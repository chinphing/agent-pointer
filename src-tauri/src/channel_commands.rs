use crate::channel_monitor::ChannelMonitorHandle;
use pointer_channels::adapters::weixin::qr_login::{QrLoginSession, QrLoginState};
use pointer_channels::config::{load_channels_config, ChannelsConfig};
use pointer_channels::ChannelGateway;
use serde::Serialize;
use std::sync::Arc;
use tauri::State;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelsStatusResponse {
    pub channels: Vec<ChannelStatusItem>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelStatusItem {
    pub channel: String,
    pub account_id: String,
    pub enabled: bool,
    pub webhook_url: String,
}

fn build_status(cfg: &ChannelsConfig) -> ChannelsStatusResponse {
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
    ChannelsStatusResponse { channels }
}

#[tauri::command]
pub fn get_channels_config() -> Result<ChannelsConfig, String> {
    load_channels_config().map_err(|e| format!("加载通道配置失败: {e:#}"))
}

#[tauri::command]
pub fn update_channels_config(
    gateway: State<'_, Arc<ChannelGateway>>,
    monitors: State<'_, Arc<ChannelMonitorHandle>>,
    cfg: ChannelsConfig,
) -> Result<(), String> {
    gateway
        .update_config(cfg)
        .map_err(|e| format!("保存通道配置失败: {e:#}"))?;
    monitors.restart();
    Ok(())
}

#[tauri::command]
pub fn list_channel_status() -> Result<ChannelsStatusResponse, String> {
    let cfg = load_channels_config().map_err(|e| format!("加载通道配置失败: {e:#}"))?;
    Ok(build_status(&cfg))
}

#[tauri::command]
pub fn get_channel_webhook_url(channel: String, account_id: String) -> Result<String, String> {
    let cfg = load_channels_config().map_err(|e| format!("加载通道配置失败: {e:#}"))?;
    Ok(cfg.webhook_url(&channel, &account_id))
}

#[tauri::command]
pub async fn start_weixin_login(
    qr: State<'_, Arc<QrLoginState>>,
    account_id: String,
) -> Result<QrLoginSession, String> {
    qr.start(&account_id)
        .await
        .map_err(|e| format!("微信扫码登录启动失败: {e:#}"))
}

#[tauri::command]
pub async fn get_weixin_login_status(
    qr: State<'_, Arc<QrLoginState>>,
    account_id: String,
) -> Result<Option<QrLoginSession>, String> {
    Ok(qr.get(&account_id).await)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingPendingResponse {
    pub pending: Vec<PairingPendingItem>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingPendingItem {
    pub code: String,
    pub sender_id: String,
}

#[tauri::command]
pub fn approve_channel_pairing(
    gateway: State<'_, Arc<ChannelGateway>>,
    channel: String,
    account_id: String,
    code: String,
) -> Result<(), String> {
    log::info!(
        "pairing approve request channel={channel} account={account_id} code={}",
        code.trim()
    );
    let ok = gateway
        .pairing
        .approve(&channel, &account_id, &code)
        .map_err(|e| format!("配对审批失败: {e:#}"))?;
    if ok {
        Ok(())
    } else {
        Err("配对码无效或已过期（请确认点击了正确通道的批准按钮，或使用最新收到的配对码）".into())
    }
}

#[tauri::command]
pub fn list_channel_pairing_pending(
    gateway: State<'_, Arc<ChannelGateway>>,
    channel: String,
    account_id: String,
) -> Result<PairingPendingResponse, String> {
    let _ = gateway.pairing.load(&channel, &account_id);
    let pending = gateway
        .pairing
        .list_pending(&channel, &account_id)
        .into_iter()
        .map(|(code, sender_id)| PairingPendingItem { code, sender_id })
        .collect();
    Ok(PairingPendingResponse { pending })
}
