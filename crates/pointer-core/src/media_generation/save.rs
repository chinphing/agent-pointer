//! Persist generated media under app data and return local paths for delivery.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::storage::{app_data_dir, sanitize_storage_dir_segment};
use crate::user_storage::{session_user_id_for_conversation, user_storage_segment};

pub const GENERATED_MEDIA_DIR: &str = "generated-media";

pub fn generated_media_root() -> Result<PathBuf> {
    Ok(app_data_dir()?.join(GENERATED_MEDIA_DIR))
}

pub fn save_generated_bytes(
    conversation_id: &str,
    bytes: &[u8],
    file_name: &str,
) -> Result<PathBuf> {
    let conv = sanitize_storage_dir_segment(conversation_id.trim());
    if conv.is_empty() {
        anyhow::bail!("conversation_id required to save generated media");
    }
    let session_user_id = session_user_id_for_conversation(conversation_id);
    let dir = generated_media_root()?
        .join(user_storage_segment(&session_user_id))
        .join(&conv);
    fs::create_dir_all(&dir).context("媒体目录创建失败")?;
    let ext = Path::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{e}"))
        .unwrap_or_default();
    let id = Uuid::new_v4();
    let path = dir.join(format!("{id}{ext}"));
    fs::write(&path, bytes).with_context(|| format!("write generated media {}", path.display()))?;
    Ok(path)
}

pub async fn download_url_to_file(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
) -> Result<()> {
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
