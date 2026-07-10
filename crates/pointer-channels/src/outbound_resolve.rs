//! Resolve outbound media path references to readable local files.

use anyhow::{Context, Result};
use pointer_core::media::path_hint::MEDIA_URI_SCHEME;
use pointer_core::media::resolve::{is_storage_rel_path, resolve_local_media_path};
use pointer_core::media::store::{is_app_data_subtree_rel, path_is_under_app_data};
use pointer_core::media::is_user_filesystem_path;
use pointer_core::media::{is_video_file_name, video::remux_video_faststart};

use crate::media::attachment::{enforce_max_bytes_for_kind, guess_mime_from_bytes};
use crate::traits::OutboundMedia;

/// Outbound kind hint for size limits (video uses Composer OSS ceiling).
fn outbound_media_kind(mime_type: &str, file_name: &str) -> &'static str {
    let mime = mime_type.trim().to_ascii_lowercase();
    if mime.starts_with("video/") || is_video_file_name(file_name) {
        "video"
    } else {
        "file"
    }
}

/// MP4/MOV containers benefit from `-movflags +faststart`; other video types are skipped.
fn should_remux_for_im_preview(mime_type: &str, file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    matches!(mime_type, "video/mp4" | "video/quicktime")
        || lower.ends_with(".mp4")
        || lower.ends_with(".mov")
}

pub struct ResolvedOutboundMedia {
    pub media: OutboundMedia,
}

pub fn resolve_outbound_media(raw: &str) -> Result<ResolvedOutboundMedia> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        anyhow::bail!("empty media path");
    }

    let rel = trimmed
        .strip_prefix(MEDIA_URI_SCHEME)
        .map(str::trim)
        .unwrap_or(trimmed);

    let path = resolve_local_media_path(rel)
        .with_context(|| format!("resolve outbound media {trimmed}"))?;
    if path.is_dir() {
        anyhow::bail!("outbound media path is a directory: {}", path.display());
    }
    if !is_deliverable_outbound_media(rel, &path) {
        anyhow::bail!(
            "outbound media path outside app data (server cannot deliver): {}",
            path.display()
        );
    }

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("attachment.bin")
        .to_string();
    let bytes = std::fs::read(&path)
        .with_context(|| format!("read outbound media {}", path.display()))?;

    let mut mime_type = mime_guess::from_path(&file_name)
        .first()
        .map(|m| m.essence_str().to_string())
        .unwrap_or_else(|| "application/octet-stream".into());
    if mime_type == "application/octet-stream" {
        if let Some(m) = guess_mime_from_bytes(&bytes) {
            mime_type = m.into();
        }
    }

    let kind = outbound_media_kind(&mime_type, &file_name);
    enforce_max_bytes_for_kind(&bytes, "outbound media", kind, &file_name, &mime_type)?;

    log::info!(
        "outbound media resolved path={} bytes={} mime={}",
        path.display(),
        bytes.len(),
        mime_type
    );

    // AI-generated videos often have the moov atom at the end, causing IM platforms
    // (Feishu, etc.) to show "0s" duration in previews. Remux with +faststart to move
    // the moov atom to the beginning — a fast stream-copy, no re-encoding.
    let bytes = if should_remux_for_im_preview(&mime_type, &file_name) {
        remux_video_faststart(&bytes, &file_name)
    } else {
        bytes
    };

    Ok(ResolvedOutboundMedia {
        media: OutboundMedia {
            file_name,
            mime_type,
            bytes,
            local_path: Some(path),
        },
    })
}

/// App-managed rel paths are deliverable once resolved; workspace paths need an app-data check.
fn is_deliverable_outbound_media(rel: &str, path: &std::path::Path) -> bool {
    if is_user_filesystem_path(rel) {
        return true;
    }
    if is_app_data_subtree_rel(rel) || is_storage_rel_path(rel) {
        return true;
    }
    path_is_under_app_data(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pointer_core::media::path_hint::MEDIA_URI_SCHEME;
    use pointer_core::media::store::{
        is_app_data_subtree_rel, path_is_under_app_data, path_under_app_data, GENERATED_MEDIA_PREFIX,
        SESSION_SANDBOXES_PREFIX,
    };
    use pointer_core::storage::app_data_dir;
    use std::fs;

    #[test]
    fn rejects_empty_path() {
        assert!(resolve_outbound_media("").is_err());
    }

    #[test]
    fn rejects_traversal() {
        assert!(resolve_outbound_media("../secret.pdf").is_err());
    }

    #[test]
    fn resolves_file_uri_as_local_path() {
        let file = std::env::temp_dir().join(format!(
            "pointer-outbound-resolve-{}.txt",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&file, b"hello").unwrap();
        let uri = format!("file://{}", file.display());
        let resolved = resolve_outbound_media(&uri).expect("resolve file uri");
        assert_eq!(resolved.media.bytes, b"hello");
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn resolves_pointer_media_generated_media_uri() {
        let root = app_data_dir().expect("app data dir");
        let rel = format!("{GENERATED_MEDIA_PREFIX}_outbound_test/conv/img.png");
        let file = root.join(&rel);
        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&file, b"png-bytes").unwrap();

        let raw = format!("{MEDIA_URI_SCHEME}{rel}");
        let resolved = resolve_outbound_media(&raw).expect("resolve generated media");
        assert_eq!(resolved.media.bytes, b"png-bytes");
        assert_eq!(resolved.media.file_name, "img.png");

        let _ = fs::remove_file(&file);
        let _ = fs::remove_dir_all(root.join("generated-media/_outbound_test"));
    }

    #[test]
    fn app_data_subtree_helpers_classify_generated_and_sandbox_paths() {
        assert!(is_app_data_subtree_rel("generated-media/u/c/a.png"));
        assert!(is_app_data_subtree_rel("session-sandboxes/u/c/a.png"));
        assert!(!is_app_data_subtree_rel("conv-id/att.png"));
        assert!(!is_user_filesystem_path("generated-media/u/c/a.png"));
    }

    #[test]
    fn deliverable_trusts_app_data_subtree_and_storage_rel() {
        assert!(is_deliverable_outbound_media(
            "generated-media/u/c/a.png",
            std::path::Path::new("/tmp/not-checked-for-subtree"),
        ));
        assert!(is_deliverable_outbound_media(
            "user/conv/a.png",
            std::path::Path::new("/tmp/not-checked-for-storage-rel"),
        ));
        assert!(!is_deliverable_outbound_media(
            "workspace-only.png",
            std::path::Path::new(std::env::temp_dir().as_path()),
        ));
    }

    #[test]
    fn path_under_app_data_accepts_generated_media_file() {
        let root = app_data_dir().expect("app data dir");
        let rel = format!("{GENERATED_MEDIA_PREFIX}_path_test/u/c/a.png");
        let file = root.join(&rel);
        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&file, b"x").unwrap();

        assert!(path_under_app_data(&file).expect("check"));
        assert!(path_is_under_app_data(&file));
        assert!(is_deliverable_outbound_media(&rel, &file));

        let _ = fs::remove_file(&file);
        let _ = fs::remove_dir_all(root.join("generated-media/_path_test"));
    }

    #[test]
    fn path_under_app_data_rejects_outside_temp_file() {
        let file = std::env::temp_dir().join(format!(
            "pointer-outside-app-data-{}.bin",
            uuid::Uuid::new_v4()
        ));
        fs::write(&file, b"x").unwrap();
        let under = path_under_app_data(&file).expect("check");
        assert!(!under);
        assert!(!path_is_under_app_data(&file));
        assert!(!is_deliverable_outbound_media("outside.bin", &file));
        let _ = fs::remove_file(&file);
    }

    #[test]
    fn session_sandbox_prefix_constant_matches_layout() {
        assert_eq!(SESSION_SANDBOXES_PREFIX, "session-sandboxes/");
    }
}
