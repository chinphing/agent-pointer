use anyhow::{Context, Result};
use base64::Engine;
use std::fs;
use std::path::{Path, PathBuf};

use crate::models::ChatMediaPreview;
use crate::storage::app_data_dir;

pub const CONVERSATION_MEDIA_DIR: &str = "conversation-media";

pub fn conversation_media_root() -> Result<PathBuf> {
    Ok(app_data_dir()?.join(CONVERSATION_MEDIA_DIR))
}

pub fn media_abs_path(storage_rel_path: &str) -> Result<PathBuf> {
    let rel = storage_rel_path.trim().trim_start_matches('/');
    if rel.is_empty() || rel.contains("..") {
        anyhow::bail!("invalid media rel path");
    }
    Ok(conversation_media_root()?.join(rel))
}

pub fn save_attachment_bytes(
    conversation_id: &str,
    attachment_id: &str,
    bytes: &[u8],
    file_name: &str,
) -> Result<String> {
    let conv = conversation_id.trim();
    let id = attachment_id.trim();
    if conv.is_empty() || id.is_empty() {
        anyhow::bail!("conversation_id and attachment_id required");
    }
    let dir = conversation_media_root()?.join(conv);
    fs::create_dir_all(&dir).context("create conversation-media dir")?;
    let ext = Path::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{e}"))
        .unwrap_or_default();
    let file_path = dir.join(format!("{id}{ext}"));
    fs::write(&file_path, bytes).context("write attachment file")?;
    Ok(format!("{conv}/{id}{ext}"))
}

pub fn read_media_bytes(storage_rel_path: &str) -> Result<Vec<u8>> {
    let path = media_abs_path(storage_rel_path)?;
    fs::read(&path).with_context(|| format!("read media file {}", path.display()))
}

pub fn read_chat_media_preview(storage_rel_path: &str) -> Result<ChatMediaPreview> {
    let path = media_abs_path(storage_rel_path)?;
    let bytes = fs::read(&path).with_context(|| format!("read media file {}", path.display()))?;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("attachment")
        .to_string();
    let mime_type = mime_from_path(&path);
    Ok(ChatMediaPreview {
        data_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
        mime_type,
        file_name,
    })
}

fn mime_from_path(path: &Path) -> String {
    match path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()) {
        Some(ext) if ext == "png" => "image/png".into(),
        Some(ext) if ext == "jpg" || ext == "jpeg" => "image/jpeg".into(),
        Some(ext) if ext == "gif" => "image/gif".into(),
        Some(ext) if ext == "webp" => "image/webp".into(),
        Some(ext) if ext == "pdf" => "application/pdf".into(),
        Some(ext) if ext == "txt" => "text/plain".into(),
        Some(ext) if ext == "md" => "text/markdown".into(),
        Some(ext) if ext == "json" => "application/json".into(),
        _ => "application/octet-stream".into(),
    }
}
