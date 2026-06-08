use anyhow::{Context, Result};
use pointer_core::local_secret;
use serde::{de::DeserializeOwned, Serialize};
use std::fs;
use std::path::PathBuf;

fn creds_dir() -> Result<PathBuf> {
    let base = dirs::data_dir().context("data dir")?;
    let dir = base
        .join(pointer_core::storage::APP_DATA_SUBDIR)
        .join("channel_credentials");
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

fn creds_path(channel: &str, account_id: &str, suffix: &str) -> Result<PathBuf> {
    Ok(creds_dir()?.join(format!("{channel}_{account_id}_{suffix}.dat")))
}

pub fn save_encrypted_json<T: Serialize>(channel: &str, account_id: &str, value: &T) -> Result<()> {
    let raw = serde_json::to_string(value)?;
    let enc = local_secret::encrypt_local_secret(&raw)?;
    fs::write(creds_path(channel, account_id, "creds")?, enc)?;
    log::info!("channel credentials saved channel={channel} account={account_id}");
    Ok(())
}

pub fn load_encrypted_json<T: DeserializeOwned>(channel: &str, account_id: &str) -> Result<Option<T>> {
    let path = creds_path(channel, account_id, "creds")?;
    if !path.exists() {
        return Ok(None);
    }
    let enc = fs::read(&path)?;
    let raw = local_secret::decrypt_local_secret(&enc)?;
    Ok(Some(serde_json::from_str(&raw)?))
}

pub fn save_sync_cursor(channel: &str, account_id: &str, cursor: &str) -> Result<()> {
    fs::write(creds_path(channel, account_id, "sync")?, cursor.as_bytes())?;
    Ok(())
}

pub fn load_sync_cursor(channel: &str, account_id: &str) -> Result<Option<String>> {
    let path = creds_path(channel, account_id, "sync")?;
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(String::from_utf8(fs::read(path)?)?))
}
