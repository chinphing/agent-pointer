use anyhow::{Context, Result};
use pointer_core::models::ChatMessage;
use std::fs;
use std::path::PathBuf;

pub struct ChannelHistoryStore;

impl ChannelHistoryStore {
    pub fn new() -> Self {
        Self
    }

    fn path(conversation_id: &str) -> Result<PathBuf> {
        let base = dirs::data_dir().context("data dir")?;
        let dir = base
            .join(pointer_core::storage::APP_DATA_SUBDIR)
            .join("channel_histories");
        if !dir.exists() {
            fs::create_dir_all(&dir)?;
        }
        let safe: String = conversation_id
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .collect();
        Ok(dir.join(format!("{safe}.json")))
    }

    pub fn load(&self, conversation_id: &str) -> Result<Vec<ChatMessage>> {
        let path = Self::path(conversation_id)?;
        if !path.exists() {
            return Ok(vec![]);
        }
        let raw = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&raw).unwrap_or_default())
    }

    pub fn save(&self, conversation_id: &str, history: &[ChatMessage]) -> Result<()> {
        fs::write(
            Self::path(conversation_id)?,
            serde_json::to_string_pretty(history)?,
        )?;
        Ok(())
    }
}
