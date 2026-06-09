//! One-time import from legacy `conversations.json`.

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension};
use std::fs;
use std::path::{Path, PathBuf};

use crate::models::Conversation;

use super::persist;

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
