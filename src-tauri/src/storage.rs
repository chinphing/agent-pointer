use crate::models::{Conversation, ModelSettings, ProviderConfig};
use anyhow::{Context, Result};
use pointer_core::storage::APP_DATA_SUBDIR;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

fn data_dir() -> Result<PathBuf> {
    let base = dirs::data_dir().context("无法获取数据目录")?;
    let dir = base.join(APP_DATA_SUBDIR);
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

fn settings_path() -> Result<PathBuf> { Ok(data_dir()?.join("settings.json")) }
fn key_path() -> Result<PathBuf> { Ok(data_dir()?.join("key.dat")) }
fn conv_path() -> Result<PathBuf> { Ok(data_dir()?.join("conversations.json")) }

#[derive(Debug, Serialize, Deserialize)]
struct StoredSettings {
    providers: Vec<StoredProvider>,
    #[serde(default, rename = "activeProviderId")]
    active_provider_id: String,
    model: String,
    temperature: f32,
    max_tokens: u32,
    #[serde(default, rename = "toolApprovalMode")]
    tool_approval_mode: String,
    #[serde(default, rename = "agentMode")]
    agent_mode: String,
    #[serde(default, rename = "workspaceRoot")]
    workspace_root: String,
    #[serde(default, rename = "leadAgentId")]
    lead_agent_id: String,
    #[serde(default, rename = "contextCompressionEnabled")]
    context_compression_enabled: bool,
    #[serde(default, rename = "contextBudgetChars")]
    context_budget_chars: u32,
    #[serde(default, rename = "contextKeepRecentUserTurns")]
    context_keep_recent_user_turns: u32,
    #[serde(default, rename = "contextSummaryMaxTokens")]
    context_summary_max_tokens: u32,
    #[serde(default, rename = "maxToolRounds")]
    max_tool_rounds: u32,
    #[serde(default = "default_raw_content_view_enabled", rename = "rawContentViewEnabled")]
    raw_content_view_enabled: bool,
    #[serde(default, rename = "agentDefaultModels")]
    agent_default_models: HashMap<String, String>,
}

fn default_raw_content_view_enabled() -> bool {
    true
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredProvider {
    id: String,
    name: String,
    #[serde(rename = "baseUrl")]
    base_url: String,
    models: Vec<String>,
}

impl Default for StoredSettings {
    fn default() -> Self {
        let s = ModelSettings::default();
        Self {
            providers: s.providers.iter().map(|p| StoredProvider {
                id: p.id.clone(),
                name: p.name.clone(),
                base_url: p.base_url.clone(),
                models: p.models.clone(),
            }).collect(),
            active_provider_id: s.active_provider_id,
            model: s.model,
            temperature: s.temperature,
            max_tokens: s.max_tokens,
            tool_approval_mode: s.tool_approval_mode,
            agent_mode: s.agent_mode,
            workspace_root: String::new(),
            lead_agent_id: String::new(),
            context_compression_enabled: true,
            context_budget_chars: 120_000,
            context_keep_recent_user_turns: 6,
            context_summary_max_tokens: 2048,
            max_tool_rounds: 100,
            raw_content_view_enabled: true,
            agent_default_models: HashMap::new(),
        }
    }
}

pub fn load_settings() -> Result<ModelSettings> {
    let path = settings_path()?;
    let stored: StoredSettings = if path.exists() {
        let raw = fs::read_to_string(&path)?;
        serde_json::from_str(&raw).unwrap_or_default()
    } else {
        StoredSettings::default()
    };

    let providers: Vec<ProviderConfig> = stored.providers.iter().map(|p| {
        ProviderConfig {
            id: p.id.clone(),
            name: p.name.clone(),
            base_url: p.base_url.clone(),
            api_key: String::new(),
            models: p.models.clone(),
        }
    }).collect();

    Ok(ModelSettings {
        providers,
        active_provider_id: if stored.active_provider_id.is_empty() { "qwen".into() } else { stored.active_provider_id },
        model: stored.model,
        api_key: String::new(),
        temperature: stored.temperature,
        max_tokens: stored.max_tokens,
        has_key: has_key()?,
        tool_approval_mode: if stored.tool_approval_mode.is_empty() { "auto".into() } else { stored.tool_approval_mode },
        agent_mode: if stored.agent_mode.is_empty() { "single".into() } else { stored.agent_mode },
        workspace_root: stored.workspace_root,
        lead_agent_id: stored.lead_agent_id,
        context_compression_enabled: stored.context_compression_enabled,
        context_budget_chars: stored.context_budget_chars,
        context_keep_recent_user_turns: stored.context_keep_recent_user_turns,
        context_summary_max_tokens: stored.context_summary_max_tokens,
        max_tool_rounds: stored.max_tool_rounds,
        raw_content_view_enabled: stored.raw_content_view_enabled,
        agent_default_models: stored.agent_default_models,
    })
}

pub fn save_settings(s: &ModelSettings) -> Result<()> {
    let stored = StoredSettings {
        providers: s.providers.iter().map(|p| StoredProvider {
            id: p.id.clone(),
            name: p.name.clone(),
            base_url: p.base_url.clone(),
            models: p.models.clone(),
        }).collect(),
        active_provider_id: s.active_provider_id.clone(),
        model: s.model.clone(),
        temperature: s.temperature,
        max_tokens: s.max_tokens,
        tool_approval_mode: s.tool_approval_mode.clone(),
        agent_mode: s.agent_mode.clone(),
        workspace_root: s.workspace_root.clone(),
        lead_agent_id: s.lead_agent_id.clone(),
        context_compression_enabled: s.context_compression_enabled,
        context_budget_chars: s.context_budget_chars,
        context_keep_recent_user_turns: s.context_keep_recent_user_turns,
        context_summary_max_tokens: s.context_summary_max_tokens,
        max_tool_rounds: s.max_tool_rounds,
        raw_content_view_enabled: s.raw_content_view_enabled,
        agent_default_models: s.agent_default_models.clone(),
    };
    fs::write(settings_path()?, serde_json::to_vec_pretty(&stored)?)?;
    Ok(())
}

/// XOR-based light obfuscation (NOT real encryption; use OS keyring in production).
fn xor_key() -> [u8; 16] {
    *b"pointer-aiwk-v1!"
}

fn obfuscate(data: &[u8]) -> Vec<u8> {
    let key = xor_key();
    data.iter().enumerate().map(|(i, b)| b ^ key[i % key.len()]).collect()
}

pub fn save_api_key(key: &str) -> Result<()> {
    let path = key_path()?;
    if key.is_empty() {
        if path.exists() { fs::remove_file(&path)?; }
        return Ok(());
    }
    fs::write(&path, obfuscate(key.as_bytes()))?;
    Ok(())
}

pub fn load_api_key() -> Result<Option<String>> {
    let path = key_path()?;
    if !path.exists() { return Ok(None); }
    let raw = fs::read(&path)?;
    let plain = obfuscate(&raw);
    Ok(Some(String::from_utf8_lossy(&plain).to_string()))
}

pub fn has_key() -> Result<bool> {
    Ok(key_path()?.exists())
}

pub fn clear_api_key() -> Result<()> {
    let path = key_path()?;
    if path.exists() { fs::remove_file(&path)?; }
    Ok(())
}

pub fn load_conversations() -> Result<Vec<Conversation>> {
    let path = conv_path()?;
    if !path.exists() { return Ok(vec![]); }
    let raw = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&raw).unwrap_or_default())
}

pub fn save_conversations(list: &[Conversation]) -> Result<()> {
    fs::write(conv_path()?, serde_json::to_vec_pretty(list)?)?;
    Ok(())
}
