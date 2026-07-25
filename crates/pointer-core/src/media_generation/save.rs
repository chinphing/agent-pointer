//! Persist generated media under app data and return local paths for delivery.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::media::store::{media_abs_path, save_attachment_bytes};

pub fn save_generated_bytes(
    conversation_id: &str,
    bytes: &[u8],
    file_name: &str,
) -> Result<PathBuf> {
    let rel = save_attachment_bytes(conversation_id, "generated", bytes, file_name)?;
    media_abs_path(&rel)
}

pub async fn download_url_to_file(client: &reqwest::Client, url: &str, dest: &Path) -> Result<()> {
    let resp = client
        .get(url)
        .send()
        .await
        .context("download generated media")?;
    if !resp.status().is_success() {
        anyhow::bail!("download failed HTTP {}", resp.status());
    }
    let bytes = resp.bytes().await.context("read download body")?;
    fs::write(dest, &bytes).with_context(|| format!("write {}", dest.display()))?;
    Ok(())
}
