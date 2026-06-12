use anyhow::{Context, Result};
use pointer_core::models::ChatMessage;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy)]
pub enum SessionArchiveReason {
    Manual,
    Idle,
}

impl SessionArchiveReason {
    fn label(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Idle => "idle",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChannelSessionMeta {
    #[serde(default)]
    pub last_interaction_at: i64,
    /// Per-IM-session lead worker (`single` mode).
    #[serde(default, rename = "leadAgentId")]
    pub lead_agent_id: Option<String>,
    /// Per-IM-session orchestration mode: `single` or `supervisor`.
    #[serde(default, rename = "agentMode")]
    pub agent_mode: Option<String>,
    /// Desktop sidebar fork counter for this IM thread (`@sN` suffix).
    #[serde(default, rename = "sessionEpoch")]
    pub session_epoch: u32,
    /// Active desktop conversation id (may include `@sN`); model context stays on base id.
    #[serde(default, rename = "activeConversationId")]
    pub active_conversation_id: Option<String>,
}

pub struct ChannelHistoryStore;

impl ChannelHistoryStore {
    pub fn new() -> Self {
        Self
    }

    fn histories_dir() -> Result<PathBuf> {
        let base = dirs::data_dir().context("data dir")?;
        let dir = base
            .join(pointer_core::storage::APP_DATA_SUBDIR)
            .join("channel_histories");
        if !dir.exists() {
            fs::create_dir_all(&dir)?;
        }
        Ok(dir)
    }

    fn archives_dir() -> Result<PathBuf> {
        let dir = Self::histories_dir()?.join("archives");
        if !dir.exists() {
            fs::create_dir_all(&dir)?;
        }
        Ok(dir)
    }

    fn safe_id(conversation_id: &str) -> String {
        conversation_id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    }

    fn history_path(conversation_id: &str) -> Result<PathBuf> {
        Ok(Self::histories_dir()?.join(format!(
            "{}.json",
            Self::safe_id(conversation_id)
        )))
    }

    fn meta_path(conversation_id: &str) -> Result<PathBuf> {
        Ok(Self::histories_dir()?.join(format!(
            "{}_meta.json",
            Self::safe_id(conversation_id)
        )))
    }

    pub fn load(&self, conversation_id: &str) -> Result<Vec<ChatMessage>> {
        let path = Self::history_path(conversation_id)?;
        if !path.exists() {
            return Ok(vec![]);
        }
        let raw = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&raw).unwrap_or_default())
    }

    pub fn save(&self, conversation_id: &str, history: &[ChatMessage]) -> Result<()> {
        fs::write(
            Self::history_path(conversation_id)?,
            serde_json::to_string_pretty(history)?,
        )?;
        Ok(())
    }

    pub fn load_meta(&self, conversation_id: &str) -> Result<ChannelSessionMeta> {
        let path = Self::meta_path(conversation_id)?;
        if !path.exists() {
            return Ok(ChannelSessionMeta::default());
        }
        let raw = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&raw).unwrap_or_default())
    }

    pub fn save_meta(&self, conversation_id: &str, meta: &ChannelSessionMeta) -> Result<()> {
        fs::write(
            Self::meta_path(conversation_id)?,
            serde_json::to_string_pretty(meta)?,
        )?;
        Ok(())
    }

    pub fn touch_meta(&self, conversation_id: &str) -> Result<()> {
        let mut meta = self.load_meta(conversation_id)?;
        meta.last_interaction_at = chrono::Utc::now().timestamp_millis();
        self.save_meta(conversation_id, &meta)
    }

    /// Move the active history file into `archives/` if it has content. Returns whether archived.
    pub fn archive(&self, conversation_id: &str, reason: SessionArchiveReason) -> Result<bool> {
        let src = Self::history_path(conversation_id)?;
        if !src.exists() {
            return Ok(false);
        }
        let raw = fs::read_to_string(&src)?;
        let history: Vec<ChatMessage> = serde_json::from_str(&raw).unwrap_or_default();
        if history.is_empty() {
            fs::remove_file(&src).ok();
            return Ok(false);
        }

        let ts = chrono::Utc::now().timestamp_millis();
        let archive_name = format!(
            "{}_{}_{}.json",
            Self::safe_id(conversation_id),
            ts,
            reason.label()
        );
        let dst = Self::archives_dir()?.join(archive_name);
        fs::rename(&src, &dst).with_context(|| {
            format!(
                "archive channel history {} -> {}",
                src.display(),
                dst.display()
            )
        })?;
        log::info!(
            "channel history archived conv={conversation_id} reason={} path={}",
            reason.label(),
            dst.display()
        );
        Ok(true)
    }

    pub fn clear(&self, conversation_id: &str) -> Result<()> {
        let path = Self::history_path(conversation_id)?;
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }

    /// Archive non-empty history (if any) and remove the active history file.
    pub fn reset_session(
        &self,
        conversation_id: &str,
        reason: SessionArchiveReason,
    ) -> Result<bool> {
        let archived = self.archive(conversation_id, reason)?;
        self.clear(conversation_id)?;
        Ok(archived)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn test_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    fn test_conv_id() -> String {
        format!("test:session_reset:{}", uuid::Uuid::new_v4())
    }

    #[test]
    fn reset_archives_and_clears_active_history() {
        let _guard = test_guard();
        let store = ChannelHistoryStore::new();
        let conv_id = test_conv_id();
        let path = ChannelHistoryStore::history_path(&conv_id).unwrap();

        store
            .save(
                &conv_id,
                &[ChatMessage {
                    id: "u1".into(),
                    role: pointer_core::models::Role::User,
                    content: "hello".into(),
                    status: "done".into(),
                    created_at: 1,
                    tool_calls: None,
                    tool_call_id: None,
                    error_message: None,
                    reasoning: None,
                    thoughts: None,
                    headline: None,
                    raw_content: None,
                    tool_raw_output: None,
                    agent_id: None,
                    agent_instance_id: None,
                    agent_name: None,
                    agent_trace: None,
                    images_base64: None,
                    image_slot_labels: None,
                    computer_round_screen_rel_path: None,
                    ui_bindings: None,
                    context_state: None,
                    attachments: None,
                }],
            )
            .unwrap();

        assert!(store.reset_session(&conv_id, SessionArchiveReason::Manual).unwrap());
        assert!(!path.exists());
        assert!(store.load(&conv_id).unwrap().is_empty());

        let archives = ChannelHistoryStore::archives_dir().unwrap();
        let archived: Vec<_> = fs::read_dir(&archives)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .contains(&ChannelHistoryStore::safe_id(&conv_id))
            })
            .collect();
        assert_eq!(archived.len(), 1);

        fs::remove_file(archived[0].path()).ok();
        fs::remove_file(ChannelHistoryStore::meta_path(&conv_id).unwrap()).ok();
    }

    #[test]
    fn touch_meta_persists_timestamp() {
        let _guard = test_guard();
        let store = ChannelHistoryStore::new();
        let conv_id = test_conv_id();

        store.touch_meta(&conv_id).unwrap();
        let meta = store.load_meta(&conv_id).unwrap();
        assert!(meta.last_interaction_at > 0);

        fs::remove_file(ChannelHistoryStore::meta_path(&conv_id).unwrap()).ok();
    }
}
