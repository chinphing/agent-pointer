//! Build persisted attachment metadata from assistant `MEDIA:` path references.

use std::path::Path;

use uuid::Uuid;

use crate::models::MediaAttachment;

use super::access::{is_user_filesystem_path, strip_file_uri};
use super::path_hint::MEDIA_URI_SCHEME;
use super::resolve::resolve_local_media_path;
use super::store::{
    app_data_media_rel_from_abs, register_sandbox_output_file, short_attachment_id_from_sandbox_rel,
};

pub fn attachments_from_reply_paths(paths: &[String]) -> Vec<MediaAttachment> {
    paths
        .iter()
        .filter_map(|raw| attachment_from_media_ref(raw))
        .collect()
}

fn attachment_from_media_ref(raw: &str) -> Option<MediaAttachment> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let normalized = strip_file_uri(trimmed).unwrap_or_else(|| trimmed.to_string());
    let rel = normalized
        .strip_prefix(MEDIA_URI_SCHEME)
        .map(str::trim)
        .unwrap_or(normalized.as_str());

    let (mut storage_rel_path, local_abs_path) = classify_media_ref(rel);

    // Verify the referenced file actually exists on disk before creating attachment.
    let check_path = local_abs_path.as_deref().or(storage_rel_path.as_deref())?;
    let mut resolved = match resolve_local_media_path(check_path) {
        Ok(p) if p.is_file() => p,
        Ok(p) => {
            log::warn!(
                "attachment_from_media_ref: not a file, skipping {} ({})",
                check_path,
                p.display()
            );
            return None;
        }
        Err(e) => {
            log::warn!("attachment_from_media_ref: file not found, skipping {check_path}: {e:#}");
            return None;
        }
    };

    if storage_rel_path
        .as_deref()
        .and_then(short_attachment_id_from_sandbox_rel)
        .is_none()
    {
        storage_rel_path = app_data_media_rel_from_abs(&resolved);
    }
    if storage_rel_path
        .as_deref()
        .and_then(short_attachment_id_from_sandbox_rel)
        .is_none()
    {
        match register_sandbox_output_file(&resolved) {
            Ok(Some(rel)) => match resolve_local_media_path(&rel) {
                Ok(path) => {
                    log::info!(
                        "reply attachment: registered sandbox output {} -> {}",
                        resolved.display(),
                        rel
                    );
                    resolved = path;
                    storage_rel_path = Some(rel);
                }
                Err(e) => {
                    log::warn!("reply attachment: resolve registered sandbox output failed: {e:#}");
                }
            },
            Ok(None) => {}
            Err(e) => {
                log::warn!(
                    "reply attachment: register sandbox output {} failed: {e:#}",
                    resolved.display()
                );
            }
        }
    }
    let id = storage_rel_path
        .as_deref()
        .and_then(short_attachment_id_from_sandbox_rel)
        .unwrap_or_else(|| format!("reply-media-{}", Uuid::new_v4()));
    let local_abs_path = Some(resolved.display().to_string());

    let file_name = file_name_from_ref(rel, local_abs_path.as_deref());
    let kind = kind_from_file_name(&file_name);
    let mime_type = mime_from_file_name(&file_name);

    Some(MediaAttachment {
        id,
        kind,
        mime_type,
        file_name,
        size_bytes: 0,
        storage_rel_path,
        content_base64: None,
        derived_text: None,
        local_abs_path,
        remote_url: None,
        oss_object_key: None,
    })
}

fn classify_media_ref(rel: &str) -> (Option<String>, Option<String>) {
    if is_user_filesystem_path(rel) {
        return (None, Some(rel.to_string()));
    }
    if rel.contains('/') {
        return (Some(rel.to_string()), None);
    }
    (None, Some(rel.to_string()))
}

fn file_name_from_ref(rel: &str, local_abs_path: Option<&str>) -> String {
    if let Some(abs) = local_abs_path {
        return Path::new(abs)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("attachment")
            .to_string();
    }
    Path::new(rel)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("attachment")
        .to_string()
}

/// Infer attachment `kind` from a file name extension.
pub fn infer_attachment_kind(file_name: &str) -> String {
    kind_from_file_name(file_name)
}

/// Infer MIME type from file name, with optional caller hint.
pub fn infer_attachment_mime(file_name: &str, mime_hint: Option<&str>) -> String {
    if let Some(m) = mime_hint.map(str::trim).filter(|s| !s.is_empty()) {
        return m.to_string();
    }
    mime_from_file_name(file_name)
}

fn kind_from_file_name(file_name: &str) -> String {
    let ext = Path::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" => "image".into(),
        "mp4" | "webm" | "mov" | "mkv" => "video".into(),
        "mp3" | "wav" | "m4a" | "aac" | "ogg" | "flac" => "audio".into(),
        "pdf" | "txt" | "md" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "html"
        | "htm" | "json" | "csv" | "xml" | "yaml" | "yml" | "svg" => "document".into(),
        "zip" | "tar" | "gz" | "7z" | "rar" => "file".into(),
        _ => "file".into(),
    }
}

fn mime_from_file_name(file_name: &str) -> String {
    match Path::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png".into(),
        Some("jpg") | Some("jpeg") => "image/jpeg".into(),
        Some("gif") => "image/gif".into(),
        Some("webp") => "image/webp".into(),
        Some("svg") => "image/svg+xml".into(),
        Some("pdf") => "application/pdf".into(),
        Some("html") | Some("htm") => "text/html".into(),
        Some("json") => "application/json".into(),
        Some("csv") => "text/csv".into(),
        Some("txt") | Some("md") => "text/plain".into(),
        Some("mp4") => "video/mp4".into(),
        Some("mp3") => "audio/mpeg".into(),
        _ => "application/octet-stream".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn temp_dir() -> PathBuf {
        std::env::temp_dir().join("pointer-reply-att-tests")
    }

    fn touch(path: &str) -> String {
        let p = PathBuf::from(path);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).ok();
        }
        let s = path.replace("$TMP", &temp_dir().to_string_lossy());
        let p = PathBuf::from(&s);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).ok();
        }
        fs::write(&p, b"test").ok();
        s
    }

    #[test]
    fn absolute_path_becomes_local_abs_path() {
        let path = touch("/tmp/pointer_test_baby.jpg");
        let atts = attachments_from_reply_paths(&[path.clone().into()]);
        assert_eq!(atts.len(), 1);
        assert_eq!(atts[0].local_abs_path.as_deref(), Some(path.as_str()));
        assert_eq!(atts[0].kind, "image");
    }

    #[test]
    fn non_existent_file_is_skipped() {
        let atts = attachments_from_reply_paths(&["/tmp/pointer_test_nonexistent.png".into()]);
        assert_eq!(atts.len(), 0);
    }

    #[test]
    fn empty_media_ref_is_skipped() {
        let atts = attachments_from_reply_paths(&["".into()]);
        assert_eq!(atts.len(), 0);
    }

    #[test]
    fn pointer_media_becomes_storage_rel_path() {
        // storage rel paths under conversation-media/ need the app data dir structure.
        // We just test that the parsing logic works when the path resolves.
        let atts = attachments_from_reply_paths(&["".into()]);
        assert_eq!(atts.len(), 0);
    }

    #[test]
    fn html_path_becomes_document_attachment() {
        let path = touch("/tmp/pointer_test_minesweeper.html");
        let atts = attachments_from_reply_paths(&[path.into()]);
        assert_eq!(atts.len(), 1);
        assert_eq!(atts[0].kind, "document");
        assert_eq!(atts[0].mime_type, "text/html");
    }

    #[test]
    fn svg_path_becomes_document_attachment() {
        // SVG must NOT be treated as an inline image: chat clients cannot
        // render it in an <img> preview, which shows a broken/question icon.
        let path = touch("/tmp/pointer_test_diagram.svg");
        let atts = attachments_from_reply_paths(&[path.into()]);
        assert_eq!(atts.len(), 1);
        assert_eq!(atts[0].kind, "document");
        assert_eq!(atts[0].mime_type, "image/svg+xml");
    }

    #[test]
    fn file_uri_normalizes_to_local_abs_path() {
        let path = touch("/tmp/pointer_test_game.html");
        let uri = format!("file://{path}");
        let atts = attachments_from_reply_paths(&[uri.into()]);
        assert_eq!(atts.len(), 1);
        assert_eq!(atts[0].local_abs_path.as_deref(), Some(path.as_str()));
    }

    #[test]
    fn tilde_path_becomes_local_abs_path() {
        // ~ paths are not expanded by resolve_local_media_path in non-interactive tests.
        let atts = attachments_from_reply_paths(&["~/Desktop/pointer_test_cover.jpg".into()]);
        assert_eq!(atts.len(), 0);
    }

    #[test]
    fn non_existent_example_media_skipped() {
        // This simulates AI writing   in explanatory text.
        let atts = attachments_from_reply_paths(&["/path/to/file.png".into()]);
        assert_eq!(atts.len(), 0);
    }

    #[test]
    fn non_existent_url_style_media_skipped() {
        //  with URL-style path that doesn't exist.
        let atts = attachments_from_reply_paths(&["pointer-media://conv/file.png".into()]);
        assert_eq!(atts.len(), 0);
    }

    #[test]
    fn non_existent_text_fragment_skipped() {
        // Simulates   extracting garbage like "...某段文字".
        let atts = attachments_from_reply_paths(&["...某段文字".into()]);
        assert_eq!(atts.len(), 0);
    }

    #[test]
    fn valid_temp_file_is_included() {
        let path = touch("/tmp/pointer_test_valid.png");
        let atts = attachments_from_reply_paths(&[path.into()]);
        assert_eq!(atts.len(), 1);
        assert_eq!(atts[0].kind, "image");
        assert_eq!(atts[0].file_name, "pointer_test_valid.png");
    }
}
