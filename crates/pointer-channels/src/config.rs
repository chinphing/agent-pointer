use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImOutboundConfig {
    /// Push assistant text to IM after each model round completes.
    #[serde(default = "default_true", rename = "sendIntermediateText")]
    pub send_intermediate_text: bool,
    /// Push tool-call progress lines to IM during agent runs.
    #[serde(default = "default_true", rename = "sendToolCalls")]
    pub send_tool_calls: bool,
}

impl Default for ImOutboundConfig {
    fn default() -> Self {
        Self {
            send_intermediate_text: true,
            send_tool_calls: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelsMeta {
    #[serde(default)]
    pub public_base_url: String,
    #[serde(default)]
    pub session_reset: SessionResetConfig,
    #[serde(default, rename = "imOutbound")]
    pub im_outbound: ImOutboundConfig,
}

impl Default for ChannelsMeta {
    fn default() -> Self {
        Self {
            public_base_url: String::new(),
            session_reset: SessionResetConfig::default(),
            im_outbound: ImOutboundConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChannelAccountConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_connection_mode")]
    pub connection_mode: String,
    // Feishu / generic
    #[serde(default, rename = "appId")]
    pub app_id: String,
    #[serde(default, rename = "appSecret")]
    pub app_secret: String,
    #[serde(default, rename = "encryptKey")]
    pub encrypt_key: String,
    #[serde(default, rename = "verificationToken")]
    pub verification_token: String,
    // DingTalk
    #[serde(default, rename = "clientId")]
    pub client_id: String,
    #[serde(default, rename = "clientSecret")]
    pub client_secret: String,
    // WeCom
    #[serde(default, rename = "corpId")]
    pub corp_id: String,
    #[serde(default, rename = "agentId")]
    pub agent_id: String,
    #[serde(default)]
    pub secret: String,
    #[serde(default)]
    pub token: String,
    #[serde(default, rename = "encodingAesKey")]
    pub encoding_aes_key: String,
    /// WeCom Bot mode (WebSocket)
    #[serde(default, rename = "botId")]
    pub bot_id: String,
    #[serde(default, rename = "websocketUrl")]
    pub websocket_url: String,
    // Policies
    #[serde(default = "default_pairing")]
    pub dm_policy: String,
    #[serde(default = "default_allowlist")]
    pub group_policy: String,
    #[serde(default = "default_true")]
    pub require_mention: bool,
    #[serde(default)]
    pub allow_from: Vec<String>,
    #[serde(default, rename = "groupAllowFrom")]
    pub group_allow_from: Vec<String>,
    /// Legacy per-account field; migrated to `meta.sessionReset` on load.
    #[serde(default, rename = "sessionReset", skip_serializing)]
    session_reset_legacy: SessionResetConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionResetConfig {
    /// Auto-reset session after this many minutes without messages. `0` = disabled; omitted = 60.
    #[serde(default, rename = "idleMinutes")]
    pub idle_minutes: Option<u32>,
}

impl SessionResetConfig {
    pub fn effective_idle_minutes(&self) -> u32 {
        match self.idle_minutes {
            Some(0) => 0,
            Some(m) => m,
            None => default_idle_minutes(),
        }
    }
}

fn default_idle_minutes() -> u32 {
    60
}

fn default_true() -> bool {
    true
}
fn default_connection_mode() -> String {
    "websocket".into()
}
fn default_pairing() -> String {
    "pairing".into()
}
fn default_allowlist() -> String {
    "allowlist".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelsConfig {
    #[serde(default)]
    pub meta: ChannelsMeta,
    #[serde(default)]
    pub feishu: HashMap<String, ChannelAccountConfig>,
    #[serde(default)]
    pub dingtalk: HashMap<String, ChannelAccountConfig>,
    #[serde(default)]
    pub wecom: HashMap<String, ChannelAccountConfig>,
    #[serde(default)]
    pub weixin: HashMap<String, ChannelAccountConfig>,
}

impl ChannelsConfig {
    pub fn account<'a>(
        &'a self,
        channel: &str,
        account_id: &str,
    ) -> Option<&'a ChannelAccountConfig> {
        match channel {
            "feishu" => self.feishu.get(account_id),
            "dingtalk" => self.dingtalk.get(account_id),
            "wecom" => self.wecom.get(account_id),
            "weixin" => self.weixin.get(account_id),
            _ => None,
        }
    }

    pub fn account_mut<'a>(
        &'a mut self,
        channel: &str,
        account_id: &str,
    ) -> Option<&'a mut ChannelAccountConfig> {
        match channel {
            "feishu" => self.feishu.get_mut(account_id),
            "dingtalk" => self.dingtalk.get_mut(account_id),
            "wecom" => self.wecom.get_mut(account_id),
            "weixin" => self.weixin.get_mut(account_id),
            _ => None,
        }
    }

    pub fn webhook_url(&self, channel: &str, account_id: &str) -> String {
        let base = self.meta.public_base_url.trim_end_matches('/');
        if base.is_empty() {
            return format!("/webhooks/{channel}/{account_id}");
        }
        format!("{base}/webhooks/{channel}/{account_id}")
    }
}

fn config_path() -> Result<PathBuf> {
    let base = dirs::data_dir().context("data dir")?;
    let dir = base.join(pointer_core::storage::APP_DATA_SUBDIR);
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    Ok(dir.join("channels_config.json"))
}

fn prefer_connection_mode(channel: &str, account: &mut ChannelAccountConfig) -> bool {
    if account.connection_mode.is_empty() {
        account.connection_mode = default_connection_mode();
        return false;
    }
    if account.connection_mode != "webhook" {
        return false;
    }
    match channel {
        "feishu" | "dingtalk" => {
            log::info!("channel {channel}: prefer websocket over saved webhook mode");
            account.connection_mode = default_connection_mode();
            true
        }
        "wecom" => {
            let ws_ready = !account.bot_id.trim().is_empty() && !account.secret.trim().is_empty();
            let webhook_only = !account.corp_id.trim().is_empty()
                && !account.agent_id.trim().is_empty()
                && !account.token.trim().is_empty()
                && !account.encoding_aes_key.trim().is_empty()
                && !ws_ready;
            if webhook_only {
                return false;
            }
            log::info!("wecom: prefer websocket (bot credentials or incomplete webhook setup)");
            account.connection_mode = default_connection_mode();
            true
        }
        _ => false,
    }
}

fn is_placeholder_credential(value: &str) -> bool {
    let v = value.trim();
    if v.is_empty() {
        return true;
    }
    let lower = v.to_lowercase();
    const EXACT: &[&str] = &["xxx", "cli_xxx", "test_encrypt_key", "test", "placeholder"];
    if EXACT.contains(&lower.as_str()) {
        return true;
    }
    lower.ends_with("_xxx")
}

fn is_placeholder_url(value: &str) -> bool {
    let lower = value.trim().to_lowercase();
    lower.contains("your-ngrok")
        || lower.contains("example.com")
        || lower.contains("pointer.example")
}

fn clear_if_placeholder(value: &mut String) -> bool {
    if is_placeholder_credential(value) {
        value.clear();
        true
    } else {
        false
    }
}

fn sanitize_account(account: &mut ChannelAccountConfig) -> bool {
    let mut changed = false;
    changed |= clear_if_placeholder(&mut account.app_id);
    changed |= clear_if_placeholder(&mut account.app_secret);
    changed |= clear_if_placeholder(&mut account.encrypt_key);
    changed |= clear_if_placeholder(&mut account.verification_token);
    changed |= clear_if_placeholder(&mut account.client_id);
    changed |= clear_if_placeholder(&mut account.client_secret);
    changed |= clear_if_placeholder(&mut account.corp_id);
    changed |= clear_if_placeholder(&mut account.agent_id);
    changed |= clear_if_placeholder(&mut account.secret);
    changed |= clear_if_placeholder(&mut account.token);
    changed |= clear_if_placeholder(&mut account.encoding_aes_key);
    changed |= clear_if_placeholder(&mut account.bot_id);
    changed |= clear_if_placeholder(&mut account.websocket_url);
    changed
}

fn sanitize_config(cfg: &mut ChannelsConfig) -> bool {
    let mut changed = false;
    for account in cfg.feishu.values_mut() {
        changed |= sanitize_account(account);
    }
    for account in cfg.dingtalk.values_mut() {
        changed |= sanitize_account(account);
    }
    for account in cfg.wecom.values_mut() {
        changed |= sanitize_account(account);
    }
    for account in cfg.weixin.values_mut() {
        changed |= sanitize_account(account);
    }
    if is_placeholder_url(&cfg.meta.public_base_url) {
        cfg.meta.public_base_url.clear();
        changed = true;
    }
    changed
}

fn migrate_session_reset(cfg: &mut ChannelsConfig) -> bool {
    if cfg.meta.session_reset.idle_minutes.is_some() {
        return false;
    }
    for accounts in [
        &cfg.feishu,
        &cfg.dingtalk,
        &cfg.wecom,
        &cfg.weixin,
    ] {
        for account in accounts.values() {
            if let Some(m) = account.session_reset_legacy.idle_minutes {
                cfg.meta.session_reset.idle_minutes = Some(m);
                return true;
            }
        }
    }
    false
}

fn normalize_config(cfg: &mut ChannelsConfig) -> bool {
    let mut migrated = false;
    for account in cfg.feishu.values_mut() {
        migrated |= prefer_connection_mode("feishu", account);
    }
    for account in cfg.dingtalk.values_mut() {
        migrated |= prefer_connection_mode("dingtalk", account);
    }
    for account in cfg.wecom.values_mut() {
        migrated |= prefer_connection_mode("wecom", account);
    }
    migrated |= migrate_session_reset(cfg);
    migrated
}

pub fn load_channels_config() -> Result<ChannelsConfig> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(ChannelsConfig::default());
    }
    let raw = fs::read_to_string(&path)?;
    let mut cfg: ChannelsConfig = serde_json::from_str(&raw).unwrap_or_default();
    let mut changed = sanitize_config(&mut cfg);
    changed |= normalize_config(&mut cfg);
    if changed {
        if let Err(e) = save_channels_config(&cfg) {
            log::warn!("failed to persist channels config cleanup: {e:#}");
        }
    }
    Ok(cfg)
}

pub fn save_channels_config(cfg: &ChannelsConfig) -> Result<()> {
    let path = config_path()?;
    let mut cfg = cfg.clone();
    sanitize_config(&mut cfg);
    normalize_config(&mut cfg);
    let raw = serde_json::to_string_pretty(&cfg)?;
    fs::write(path, raw)?;
    Ok(())
}

/// Whether the channel monitor is actively connected (runtime state).
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn im_outbound_defaults() {
        let cfg: ImOutboundConfig = serde_json::from_str("{}").unwrap();
        assert!(cfg.send_intermediate_text);
        assert!(cfg.send_tool_calls);
    }

    #[test]
    fn session_reset_defaults_to_one_hour() {
        assert_eq!(SessionResetConfig::default().effective_idle_minutes(), 60);
        assert_eq!(
            SessionResetConfig {
                idle_minutes: Some(0)
            }
            .effective_idle_minutes(),
            0
        );
        assert_eq!(
            SessionResetConfig {
                idle_minutes: Some(30)
            }
            .effective_idle_minutes(),
            30
        );
    }
}

pub fn account_runtime_connected(
    channel: &str,
    account_id: &str,
    account: &ChannelAccountConfig,
) -> bool {
    if !account.enabled {
        return false;
    }
    if matches!(channel, "feishu" | "dingtalk" | "wecom")
        && account.connection_mode != "websocket"
    {
        return false;
    }
    crate::connection_state::is_connected(channel, account_id)
}
