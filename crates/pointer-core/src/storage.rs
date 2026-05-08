use crate::models::{Conversation, ModelSettings, ProviderConfig};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const APP_DIR: &str = "PointerApp";

fn data_dir() -> Result<PathBuf> {
    let base = dirs::data_dir().context("无法获取数据目录")?;
    let dir = base.join(APP_DIR);
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

pub fn app_data_dir() -> Result<PathBuf> {
    data_dir()
}

fn settings_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("settings.json"))
}
fn key_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("key.dat"))
}
fn conv_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("conversations.json"))
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredProvider {
    id: String,
    name: String,
    #[serde(rename = "baseUrl")]
    base_url: String,
    models: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredSettings {
    providers: Vec<StoredProvider>,
    #[serde(default, rename = "activeProviderId")]
    active_provider_id: String,
    model: String,
    temperature: f32,
    max_tokens: u32,
    #[serde(default = "default_tool_approval_mode")]
    tool_approval_mode: String,
    #[serde(default = "default_agent_mode")]
    agent_mode: String,
}

fn default_tool_approval_mode() -> String {
    "auto".into()
}

fn default_agent_mode() -> String {
    "single".into()
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

    let active_provider_id = if stored.active_provider_id.is_empty() {
        "qwen".into()
    } else {
        stored.active_provider_id
    };

    Ok(ModelSettings {
        providers,
        active_provider_id,
        model: stored.model,
        api_key: String::new(),
        temperature: stored.temperature,
        max_tokens: stored.max_tokens,
        has_key: has_key()?,
        tool_approval_mode: stored.tool_approval_mode,
        agent_mode: stored.agent_mode,
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
    data.iter()
        .enumerate()
        .map(|(i, b)| b ^ key[i % key.len()])
        .collect()
}

pub fn save_api_key(key: &str) -> Result<()> {
    let path = key_path()?;
    if key.is_empty() {
        if path.exists() {
            fs::remove_file(&path)?;
        }
        return Ok(());
    }
    fs::write(&path, obfuscate(key.as_bytes()))?;
    Ok(())
}

pub fn load_api_key() -> Result<Option<String>> {
    let path = key_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read(&path)?;
    let plain = obfuscate(&raw);
    Ok(Some(String::from_utf8_lossy(&plain).to_string()))
}

pub fn has_key() -> Result<bool> {
    Ok(key_path()?.exists())
}

pub fn clear_api_key() -> Result<()> {
    let path = key_path()?;
    if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}

pub fn load_conversations() -> Result<Vec<Conversation>> {
    let path = conv_path()?;
    if !path.exists() {
        return Ok(vec![]);
    }
    let raw = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&raw).unwrap_or_default())
}

pub fn save_conversations(list: &[Conversation]) -> Result<()> {
    fs::write(conv_path()?, serde_json::to_vec_pretty(list)?)?;
    Ok(())
}
