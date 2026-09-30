//! One-time import from legacy `conversations.json`.

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension};
use std::fs;
use std::path::{Path, PathBuf};

use crate::models::Conversation;
use crate::storage::app_data_dir;

use super::im_session;
use super::persist;
use super::write;

pub fn migrate_json_if_needed(conn: &Connection, json_path: &Path) -> Result<()> {
    if meta_flag(conn, "json_migrated")? {
        return Ok(());
    }
    if !json_path.exists() {
        set_meta(conn, "json_migrated", "1")?;
        log::info!("conversation_store: no legacy conversations.json; starting fresh");
        return Ok(());
    }
    let raw = fs::read_to_string(json_path)
        .with_context(|| format!("read legacy {}", json_path.display()))?;
    let convs: Vec<Conversation> = serde_json::from_str(&raw).unwrap_or_default();
    log::info!(
        "conversation_store: importing {} conversations from {}",
        convs.len(),
        json_path.display()
    );
    persist::replace_all_in_conn(conn, &convs)?;
    set_meta(conn, "json_migrated", "1")?;
    let backup = json_path.with_extension("json.migrated");
    if backup.exists() {
        let _ = fs::remove_file(&backup);
    }
    fs::rename(json_path, &backup).with_context(|| {
        format!(
            "archive legacy json {} -> {}",
            json_path.display(),
            backup.display()
        )
    })?;
    log::info!(
        "conversation_store: archived legacy json to {}",
        backup.display()
    );
    Ok(())
}

pub fn default_json_path() -> Result<PathBuf> {
    Ok(crate::storage::app_data_dir()?.join("conversations.json"))
}

fn meta_flag(conn: &Connection, key: &str) -> Result<bool> {
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM store_meta WHERE key = ?1",
            [key],
            |row| row.get(0),
        )
        .optional()?;
    Ok(value.as_deref() == Some("1"))
}

fn set_meta(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO store_meta(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        (key, value),
    )?;
    Ok(())
}

/// Deprecate legacy `channel_histories/` JSON (session state now lives in SQLite).
pub fn migrate_channel_histories_if_needed(conn: &Connection) -> Result<()> {
    if meta_flag(conn, "channel_histories_migrated")? {
        return Ok(());
    }
    let dir = app_data_dir()?.join("channel_histories");
    if dir.is_dir() {
        let deprecated = dir.with_file_name("channel_histories.deprecated");
        if deprecated.exists() {
            let _ = fs::remove_dir_all(&deprecated);
        }
        if let Err(e) = fs::rename(&dir, &deprecated) {
            log::warn!("conversation_store: could not rename legacy channel_histories: {e:#}");
        } else {
            log::info!(
                "conversation_store: deprecated legacy channel_histories at {}",
                deprecated.display()
            );
            import_legacy_channel_meta(conn, &deprecated)?;
        }
    }
    set_meta(conn, "channel_histories_migrated", "1")?;
    Ok(())
}

fn import_legacy_channel_meta(conn: &Connection, dir: &Path) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        if !name.ends_with("_meta.json") {
            continue;
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("read legacy channel meta {}", path.display()))?;
        let legacy: LegacyChannelMeta = serde_json::from_str(&raw).unwrap_or_default();
        let base = legacy
            .active_conversation_id
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(crate::channel_outbound::im_base_conversation_id);
        let Some(base) = base else {
            log::warn!(
                "conversation_store: skip legacy channel meta without activeConversationId: {}",
                path.display()
            );
            continue;
        };
        write::ensure_conversation_row(conn, &base)?;
        let state = im_session::ImSessionState {
            session_epoch: legacy.session_epoch,
            active_conversation_id: legacy
                .active_conversation_id
                .filter(|s| !s.trim().is_empty()),
            last_interaction_at_ms: legacy.last_interaction_at,
            lead_agent_id: legacy
                .lead_agent_id
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| crate::agents::DEFAULT_LEAD_AGENT_ID.to_string()),
        };
        im_session::save_im_session_in_conn(conn, &base, &state)?;
        log::info!(
            "conversation_store: imported legacy IM session meta for base={base} epoch={}",
            state.session_epoch
        );
    }
    Ok(())
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyChannelMeta {
    #[serde(default)]
    last_interaction_at: i64,
    #[serde(default, rename = "leadAgentId")]
    lead_agent_id: Option<String>,
    #[serde(default, rename = "sessionEpoch")]
    session_epoch: u32,
    #[serde(default, rename = "activeConversationId")]
    active_conversation_id: Option<String>,
}
