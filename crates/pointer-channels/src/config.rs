use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChannelsMeta {
    #[serde(default)]
    pub public_base_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChannelAccountConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_webhook_mode")]
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
}

fn default_true() -> bool {
    true
}
fn default_webhook_mode() -> String {
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

pub fn load_channels_config() -> Result<ChannelsConfig> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(ChannelsConfig::default());
    }
    let raw = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&raw).unwrap_or_default())
}

pub fn save_channels_config(cfg: &ChannelsConfig) -> Result<()> {
    let path = config_path()?;
    let raw = serde_json::to_string_pretty(cfg)?;
    fs::write(path, raw)?;
    Ok(())
}
