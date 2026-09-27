use crate::channel_monitor::ChannelMonitorHandle;
use pointer_channels::adapters::weixin::ilink_client::WeixinCredentials;
use pointer_channels::adapters::weixin::qr_login::{QrLoginSession, QrLoginState};
use pointer_channels::config::{account_runtime_connected, load_channels_config, ChannelsConfig};
use pointer_channels::credentials::load_encrypted_json;
use pointer_channels::registration::{ChannelRegistrationState, RegistrationSession};
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
    pub connected: bool,
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
                connected: account_runtime_connected(channel, account_id, account),
                webhook_url: cfg.webhook_url(channel, account_id),
            });
        }
    }
    ChannelsStatusResponse { channels }
}

fn ui_err(key: &str) -> String {
    pointer_core::i18n::t(key, pointer_core::i18n::current_ui_locale()).to_string()
}

fn ui_errf(key: &str, args: &[(&str, &str)]) -> String {
    pointer_core::i18n::tf(key, pointer_core::i18n::current_ui_locale(), args)
}

#[tauri::command]
pub fn get_channels_config() -> Result<ChannelsConfig, String> {
    load_channels_config()
        .map_err(|e| ui_errf("err.channels_load_failed", &[("e", &format!("{e:#}"))]))
}

#[tauri::command]
pub fn update_channels_config(
    gateway: State<'_, Arc<ChannelGateway>>,
    monitors: State<'_, Arc<ChannelMonitorHandle>>,
    cfg: ChannelsConfig,
    restart_monitors: Option<bool>,
) -> Result<(), String> {
    if restart_monitors.unwrap_or(false) {
        gateway
            .update_config_and_restart(cfg, monitors.supervisor().as_ref())
            .map_err(|e| ui_errf("err.channels_save_failed", &[("e", &format!("{e:#}"))]))?;
        log::info!("channel monitors restarted after explicit connect");
    } else {
        gateway
            .update_config(cfg)
            .map_err(|e| ui_errf("err.channels_save_failed", &[("e", &format!("{e:#}"))]))?;
    }
    Ok(())
}

#[tauri::command]
pub fn list_channel_status() -> Result<ChannelsStatusResponse, String> {
    let cfg = load_channels_config()
        .map_err(|e| ui_errf("err.channels_load_failed", &[("e", &format!("{e:#}"))]))?;
    Ok(build_status(&cfg))
}

#[tauri::command]
pub fn get_channel_webhook_url(channel: String, account_id: String) -> Result<String, String> {
    let cfg = load_channels_config()
        .map_err(|e| ui_errf("err.channels_load_failed", &[("e", &format!("{e:#}"))]))?;
    Ok(cfg.webhook_url(&channel, &account_id))
}

#[tauri::command]
pub async fn start_weixin_login(
    qr: State<'_, Arc<QrLoginState>>,
    account_id: String,
) -> Result<QrLoginSession, String> {
    qr.start(&account_id)
        .await
        .map_err(|e| ui_errf("err.weixin_login_start_failed", &[("e", &format!("{e:#}"))]))
}

#[tauri::command]
pub async fn get_weixin_login_status(
    qr: State<'_, Arc<QrLoginState>>,
    account_id: String,
) -> Result<Option<QrLoginSession>, String> {
    Ok(qr.get(&account_id).await)
}

#[tauri::command]
pub fn has_weixin_credentials(account_id: String) -> Result<bool, String> {
    let creds = load_encrypted_json::<WeixinCredentials>("weixin", &account_id).map_err(|e| {
        ui_errf(
            "err.weixin_credentials_read_failed",
            &[("e", &format!("{e:#}"))],
        )
    })?;
    Ok(creds.is_some())
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
    pub issued_at: i64,
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
        .map_err(|e| ui_errf("err.pairing_approve_failed", &[("e", &format!("{e:#}"))]))?;
    if ok {
        Ok(())
    } else {
        Err(ui_err("err.pairing_code_invalid"))
    }
}

#[tauri::command]
pub async fn start_channel_registration(
    registration: State<'_, Arc<ChannelRegistrationState>>,
    channel: String,
    account_id: String,
) -> Result<RegistrationSession, String> {
    registration
        .start(&channel, &account_id)
        .await
        .map_err(|e| {
            ui_errf(
                "err.channel_registration_start_failed",
                &[("channel", &channel), ("e", &format!("{e:#}"))],
            )
        })
}

#[tauri::command]
pub async fn get_channel_registration_status(
    registration: State<'_, Arc<ChannelRegistrationState>>,
    channel: String,
    account_id: String,
) -> Result<Option<RegistrationSession>, String> {
    Ok(registration.get(&channel, &account_id).await)
}

#[tauri::command]
pub fn list_channel_pairing_pending(
    gateway: State<'_, Arc<ChannelGateway>>,
    channel: String,
    account_id: String,
) -> Result<PairingPendingResponse, String> {
    let pending = gateway
        .pairing
        .list_pending(&channel, &account_id)
        .into_iter()
        .map(|(code, sender_id, issued_at)| PairingPendingItem {
            code,
            sender_id,
            issued_at,
        })
        .collect();
    Ok(PairingPendingResponse { pending })
}
