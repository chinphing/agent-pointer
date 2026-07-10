//! Resolve outbound media path references to readable local files.

use anyhow::{Context, Result};
use pointer_core::media::path_hint::MEDIA_URI_SCHEME;
use pointer_core::media::public_download::{
    configured_ttl_secs, issue_download_token, PUBLIC_DOWNLOAD_MAX_BYTES,
};
use pointer_core::media::resolve::{is_storage_rel_path, resolve_local_media_path};
use pointer_core::media::store::{
    app_data_media_rel_from_abs, is_app_data_subtree_rel, path_is_under_app_data,
};
use pointer_core::media::is_user_filesystem_path;
use pointer_core::media::{is_video_file_name, video::remux_video_faststart};

use crate::config::resolve_im_public_base_url;

use crate::media::attachment::{
    channel_media_max_bytes, enforce_max_bytes_for_kind, guess_mime_from_bytes,
    CHANNEL_MEDIA_MAX_BYTES,
};
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

/// WeCom (and most IM bots) reject ordinary file uploads above ~20 MB (`errcode=40006`).
/// Keep IM direct-send under this ceiling; larger files use a signed public download URL.
pub const IM_DIRECT_SEND_MAX_BYTES: usize = 20 * 1024 * 1024;

/// Max bytes for IM **direct** media upload (platform attachment). Larger files use a
/// signed public download URL instead. Capped at [`IM_DIRECT_SEND_MAX_BYTES`] so WeCom
/// does not accept a 21 MB file into upload_init and then fail with 40006.
fn im_direct_send_max_bytes(kind: &str, file_name: &str, mime_type: &str) -> usize {
    let full = channel_media_max_bytes(kind, file_name, mime_type);
    full.min(IM_DIRECT_SEND_MAX_BYTES)
        .min(CHANNEL_MEDIA_MAX_BYTES)
}

pub struct ResolvedOutboundMedia {
    pub media: OutboundMedia,
}

/// How to deliver one outbound media ref on IM channels.
#[derive(Debug)]
pub enum ImOutboundMediaDelivery {
    /// Upload bytes as a native IM attachment.
    Direct(OutboundMedia),
    /// File too large for IM upload; send a time-limited HTTPS download link instead.
    DownloadLink {
        url: String,
        file_name: String,
        size_bytes: u64,
    },
}

pub fn resolve_outbound_media(raw: &str) -> Result<ResolvedOutboundMedia> {
    let prepared = prepare_outbound_file(raw)?;
    enforce_max_bytes_for_kind(
        &prepared.bytes,
        "outbound media",
        &prepared.kind,
        &prepared.file_name,
        &prepared.mime_type,
    )?;
    let bytes = maybe_remux(prepared.bytes, &prepared.mime_type, &prepared.file_name);
    Ok(ResolvedOutboundMedia {
        media: OutboundMedia {
            file_name: prepared.file_name,
            mime_type: prepared.mime_type,
            bytes,
            local_path: Some(prepared.path),
        },
    })
}

/// Resolve media for IM: direct upload when small enough, else signed public download URL.
pub fn resolve_im_outbound_media(raw: &str) -> Result<ImOutboundMediaDelivery> {
    let prepared = prepare_outbound_file(raw)?;
    let direct_max = im_direct_send_max_bytes(&prepared.kind, &prepared.file_name, &prepared.mime_type);
    if prepared.bytes.len() <= direct_max {
        let bytes = maybe_remux(prepared.bytes, &prepared.mime_type, &prepared.file_name);
        return Ok(ImOutboundMediaDelivery::Direct(OutboundMedia {
            file_name: prepared.file_name,
            mime_type: prepared.mime_type,
            bytes,
            local_path: Some(prepared.path),
        }));
    }

    if prepared.bytes.len() as u64 > PUBLIC_DOWNLOAD_MAX_BYTES {
        anyhow::bail!(
            "outbound media exceeds public download limit ({} MB)",
            PUBLIC_DOWNLOAD_MAX_BYTES / (1024 * 1024)
        );
    }
    if resolve_im_public_base_url().is_none() {
        anyhow::bail!(
            "outbound media exceeds IM direct limit ({} MB) and no public URL is configured \
             (set POINTER_SERVER_PUBLIC_URL or channels publicBaseUrl); cannot issue download link",
            direct_max / (1024 * 1024)
        );
    }
    let path_for_token = app_data_media_rel_from_abs(&prepared.path)
        .unwrap_or_else(|| prepared.path.display().to_string());
    let token = issue_download_token(
        &path_for_token,
        Some(&prepared.file_name),
        configured_ttl_secs(),
    )
    .with_context(|| format!("issue public download token for {}", prepared.file_name))?;
    let base = resolve_im_public_base_url().expect("checked above");
    let url = format!(
        "{base}/api/media/public-download?token={}",
        urlencoding::encode(&token)
    );
    log::info!(
        "im outbound media oversized path={} bytes={} -> public download link",
        prepared.path.display(),
        prepared.bytes.len()
    );
    Ok(ImOutboundMediaDelivery::DownloadLink {
        url,
        file_name: prepared.file_name,
        size_bytes: prepared.bytes.len() as u64,
    })
}

/// Force a public download link for an already-resolved path (upload failure fallback).
pub fn try_im_download_link_for_path(raw: &str) -> Result<ImOutboundMediaDelivery> {
    let prepared = prepare_outbound_file(raw)?;
    if prepared.bytes.len() as u64 > PUBLIC_DOWNLOAD_MAX_BYTES {
        anyhow::bail!(
            "outbound media exceeds public download limit ({} MB)",
            PUBLIC_DOWNLOAD_MAX_BYTES / (1024 * 1024)
        );
    }
    let Some(base) = resolve_im_public_base_url() else {
        anyhow::bail!(
            "no public URL configured (set POINTER_SERVER_PUBLIC_URL or channels publicBaseUrl)"
        );
    };
    let path_for_token = app_data_media_rel_from_abs(&prepared.path)
        .unwrap_or_else(|| prepared.path.display().to_string());
    let token = issue_download_token(
        &path_for_token,
        Some(&prepared.file_name),
        configured_ttl_secs(),
    )
    .with_context(|| format!("issue public download token for {}", prepared.file_name))?;
    let url = format!(
        "{base}/api/media/public-download?token={}",
        urlencoding::encode(&token)
    );
    log::info!(
        "im outbound media upload-fallback path={} bytes={} -> public download link",
        prepared.path.display(),
        prepared.bytes.len()
    );
    Ok(ImOutboundMediaDelivery::DownloadLink {
        url,
        file_name: prepared.file_name,
        size_bytes: prepared.bytes.len() as u64,
    })
}

struct PreparedOutboundFile {
    path: std::path::PathBuf,
    file_name: String,
    mime_type: String,
    kind: String,
    bytes: Vec<u8>,
}

fn prepare_outbound_file(raw: &str) -> Result<PreparedOutboundFile> {
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

    let kind = outbound_media_kind(&mime_type, &file_name).to_string();
    log::info!(
        "outbound media prepared path={} bytes={} mime={}",
        path.display(),
        bytes.len(),
        mime_type
    );
    Ok(PreparedOutboundFile {
        path,
        file_name,
        mime_type,
        kind,
        bytes,
    })
}

fn maybe_remux(bytes: Vec<u8>, mime_type: &str, file_name: &str) -> Vec<u8> {
    // AI-generated videos often have the moov atom at the end, causing IM platforms
    // (Feishu, etc.) to show "0s" duration in previews. Remux with +faststart to move
    // the moov atom to the beginning — a fast stream-copy, no re-encoding.
    if should_remux_for_im_preview(mime_type, file_name) {
        remux_video_faststart(&bytes, file_name)
    } else {
        bytes
    }
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

pub fn format_im_download_link_message(file_name: &str, size_bytes: u64, url: &str) -> String {
    let size_label = if size_bytes >= 1024 * 1024 {
        format!("{:.1} MB", size_bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{} KB", (size_bytes / 1024).max(1))
    };
    format!("📎 {file_name} ({size_label})\n下载：{url}")
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

    #[test]
    fn download_link_message_includes_name_and_url() {
        let msg = format_im_download_link_message(
            "report.pdf",
            40 * 1024 * 1024,
            "https://example.com/api/media/public-download?token=abc",
        );
        assert!(msg.contains("report.pdf"));
        assert!(msg.contains("40.0 MB"));
        assert!(msg.contains("https://example.com/api/media/public-download?token=abc"));
    }

    #[test]
    fn im_direct_send_cap_is_20mb() {
        assert_eq!(IM_DIRECT_SEND_MAX_BYTES, 20 * 1024 * 1024);
        assert_eq!(
            im_direct_send_max_bytes("file", "a.zip", "application/zip"),
            20 * 1024 * 1024
        );
        // 21.12 MB must not go Direct — would hit WeCom 40006.
        assert!(21_123_393 > IM_DIRECT_SEND_MAX_BYTES);
    }
}
