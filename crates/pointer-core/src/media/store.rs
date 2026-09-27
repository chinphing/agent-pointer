use anyhow::{Context, Result};
use base64::Engine;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::models::ChatMediaPreview;
use crate::session_sandbox::{SessionSandbox, ANONYMOUS_SEGMENT};
use crate::storage::app_data_dir;
use crate::user_storage::session_user_id_for_conversation;

use super::access::{assert_app_media_preview_allowed, path_has_traversal};
use super::filename::safe_attachment_basename;
use super::layout::{parse_storage_rel, verify_storage_rel_access};
use super::path_hint::MEDIA_URI_SCHEME;
use super::resolve::resolve_local_media_path;

pub const CONVERSATION_MEDIA_DIR: &str = "conversation-media";

/// App-data subtrees outside `conversation-media/` that use their own rel prefix.
pub const GENERATED_MEDIA_PREFIX: &str = "generated-media/";
pub const SESSION_SANDBOXES_PREFIX: &str = "session-sandboxes/";
pub const SANDBOX_ATTACHMENTS_DIR: &str = "attachments";
pub const SHORT_ATTACHMENT_ID_LEN: usize = 12;

/// Non-video composer uploads use the user setting (default 100 MB). Videos keep
/// the OSS/local video ceiling.
pub fn composer_attachment_max_bytes(file_name: &str) -> u64 {
    if super::video::is_video_file_name(file_name) {
        super::video::MAX_VIDEO_BYTES as u64
    } else {
        crate::models::clamp_attachment_upload_max_bytes(
            crate::storage::load_user_settings()
                .unwrap_or_default()
                .attachment_upload_max_bytes,
        ) as u64
    }
}

pub fn ensure_composer_attachment_size(len: u64, file_name: &str) -> Result<()> {
    let limit = composer_attachment_max_bytes(file_name);
    if len > limit {
        let limit_mb = (limit / (1024 * 1024)).max(1);
        let msg = crate::i18n::tf(
            "err.file_too_large",
            crate::i18n::current_ui_locale(),
            &[("limit", &limit_mb.to_string())],
        );
        anyhow::bail!(msg);
    }
    Ok(())
}

/// True for rel paths stored directly under `{app_data}/` (not conversation attachment layout).
pub fn is_app_data_subtree_rel(raw: &str) -> bool {
    let rel = raw.trim().trim_start_matches('/');
    rel.starts_with(GENERATED_MEDIA_PREFIX) || rel.starts_with(SESSION_SANDBOXES_PREFIX)
}

/// Return a 12-character lowercase hex attachment ID embedded in a sandbox attachment path.
pub fn short_attachment_id_from_sandbox_rel(raw: &str) -> Option<String> {
    let rel = raw.trim().trim_start_matches('/');
    let file_name = Path::new(rel).file_name()?.to_str()?;
    let (id, _) = file_name.split_once('_')?;
    if id.len() == SHORT_ATTACHMENT_ID_LEN && id.bytes().all(|b| b.is_ascii_hexdigit()) {
        Some(id.to_ascii_lowercase())
    } else {
        None
    }
}

fn sandbox_attachment_rel(conversation_id: &str) -> Result<(PathBuf, String)> {
    let session_user_id = session_user_id_for_conversation(conversation_id);
    let sandbox = SessionSandbox::ensure_default(conversation_id, &session_user_id)?;
    let dir = sandbox.join(SANDBOX_ATTACHMENTS_DIR);
    fs::create_dir_all(&dir).context("create sandbox attachment directory")?;
    let rel = dir
        .strip_prefix(app_data_dir()?)
        .context("sandbox attachment outside app data")?
        .to_string_lossy()
        .replace('\\', "/");
    Ok((dir, rel))
}

fn sandbox_attachment_dir_for_path(path: &Path) -> Result<Option<PathBuf>> {
    let root = app_data_dir()?;
    let rel = match path.strip_prefix(&root) {
        Ok(rel) => rel,
        Err(_) => return Ok(None),
    };
    let parts: Vec<_> = rel.components().collect();
    if parts.first().and_then(|part| part.as_os_str().to_str()) != Some("session-sandboxes") {
        return Ok(None);
    }
    let user = parts
        .get(1)
        .ok_or_else(|| anyhow::anyhow!("sandbox path missing user segment"))?;
    let mut sandbox = root.join("session-sandboxes").join(user.as_os_str());
    if user.as_os_str() == std::ffi::OsStr::new(ANONYMOUS_SEGMENT) {
        let conversation = parts.get(2).ok_or_else(|| {
            anyhow::anyhow!("anonymous sandbox path missing conversation segment")
        })?;
        sandbox.push(conversation.as_os_str());
    }
    Ok(Some(sandbox.join(SANDBOX_ATTACHMENTS_DIR)))
}

/// Copy a file created in a default sandbox into its attachment directory and assign a short ID.
///
/// Files already in `attachments/` are returned unchanged. Files outside a default sandbox are
/// ignored so caller-owned paths are never copied without an explicit conversation context.
pub fn register_sandbox_output_file(path: &Path) -> Result<Option<String>> {
    let source = path
        .canonicalize()
        .with_context(|| format!("resolve sandbox output {}", path.display()))?;
    let Some(dir) = sandbox_attachment_dir_for_path(&source)? else {
        return Ok(None);
    };
    if source.starts_with(&dir)
        && app_data_media_rel_from_abs(&source)
            .as_deref()
            .and_then(short_attachment_id_from_sandbox_rel)
            .is_some()
    {
        return Ok(app_data_media_rel_from_abs(&source));
    }
    if !source.is_file() {
        anyhow::bail!("sandbox output is not a file: {}", source.display());
    }
    fs::create_dir_all(&dir).context("create sandbox attachment directory")?;
    let source_name = source
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("attachment");
    let (_, dest, mut file) = create_sandbox_attachment_file(&dir, source_name)?;
    match fs::read(&source)
        .with_context(|| format!("read sandbox output {}", source.display()))
        .and_then(|bytes| {
            file.write_all(&bytes)
                .with_context(|| format!("write sandbox attachment {}", dest.display()))
        }) {
        Ok(()) => app_data_media_rel_from_abs(&dest)
            .map(Some)
            .ok_or_else(|| anyhow::anyhow!("sandbox attachment outside app data")),
        Err(e) => {
            let _ = fs::remove_file(&dest);
            Err(e)
        }
    }
}

fn next_short_attachment_id() -> u64 {
    let raw = Uuid::new_v4().simple().to_string();
    u64::from_str_radix(&raw[..SHORT_ATTACHMENT_ID_LEN], 16).unwrap_or(0)
}

/// Reserve an attachment filename atomically. Collisions advance to the next 48-bit value.
fn create_sandbox_attachment_file(
    dir: &Path,
    file_name: &str,
) -> Result<(String, PathBuf, std::fs::File)> {
    let safe_name = safe_attachment_basename(file_name);
    let display_name = if safe_name.is_empty() {
        "attachment".to_string()
    } else {
        safe_name
    };
    let mut value = next_short_attachment_id();
    for _ in 0..=0xFF_FFFF {
        let id = format!("{value:0width$x}", width = SHORT_ATTACHMENT_ID_LEN);
        let path = dir.join(format!("{id}_{display_name}"));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((id, path, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                value = (value + 1) & 0xFF_FFFF_FFFF_FFFF;
            }
            Err(e) => {
                return Err(e)
                    .with_context(|| format!("reserve sandbox attachment {}", path.display()));
            }
        }
    }
    anyhow::bail!("unable to allocate a unique 12-character attachment ID")
}

pub fn conversation_media_root() -> Result<PathBuf> {
    Ok(app_data_dir()?.join(CONVERSATION_MEDIA_DIR))
}

pub fn media_abs_path(storage_rel_path: &str) -> Result<PathBuf> {
    if is_app_data_subtree_rel(storage_rel_path) {
        let path = app_data_dir()?.join(storage_rel_path.trim().trim_start_matches('/'));
        if path.exists() {
            return Ok(path);
        }
        anyhow::bail!("media file not found under app data: {}", path.display());
    }
    if let Err(e) = verify_storage_rel_access(storage_rel_path) {
        log::info!("media_abs_path verify_storage_rel_access FAILED: {e:#}");
        return Err(e);
    }
    media_abs_path_unscoped(storage_rel_path)
}

/// Resolve a storage rel path without session-user scoping (server outbound / allowlisted reads).
pub fn media_abs_path_unscoped(storage_rel_path: &str) -> Result<PathBuf> {
    let rel = storage_rel_path.trim().trim_start_matches('/');
    if rel.is_empty() || rel.contains("..") {
        anyhow::bail!("invalid media rel path");
    }
    let primary = conversation_media_root()?.join(rel);
    if primary.is_file() {
        return Ok(primary);
    }
    // Legacy `{conv}/{file}` paths remain readable after user-scoped layout migration.
    if let Ok(parsed) = parse_storage_rel(rel) {
        if !parsed.legacy {
            let legacy = conversation_media_root()?
                .join(&parsed.conversation_segment)
                .join(&parsed.file_name);
            if legacy.is_file() {
                return Ok(legacy);
            }
        }
    }
    // `generated-media/…` and other app-data subtrees (not under conversation-media/).
    if let Ok(data_dir) = app_data_dir() {
        let under_data = data_dir.join(rel);
        if under_data.is_file() {
            return Ok(under_data);
        }
    }
    Ok(primary)
}

/// True when `path` resolves under the Pointer app data directory.
pub fn path_under_app_data(path: &Path) -> Result<bool> {
    let root = app_data_dir()?.canonicalize().context("app data dir")?;
    let canonical = path
        .canonicalize()
        .with_context(|| format!("resolve media path {}", path.display()))?;
    Ok(canonical.starts_with(&root))
}

/// Best-effort check for outbound / preview guards; falls back to non-canonical prefix match.
pub fn path_is_under_app_data(path: &Path) -> bool {
    match path_under_app_data(path) {
        Ok(under) => under,
        Err(e) => {
            log::warn!(
                "path_is_under_app_data canonicalize failed path={}: {e:#}; trying prefix check",
                path.display()
            );
            app_data_dir()
                .ok()
                .is_some_and(|root| path.starts_with(&root))
        }
    }
}

/// Map an on-disk file under app data to a storage rel path for API / IM delivery.
pub fn app_data_media_rel_from_abs(path: &Path) -> Option<String> {
    let data_dir = app_data_dir().ok()?;
    let abs = path.canonicalize().ok()?;
    let root = data_dir.canonicalize().ok()?;
    let rel = abs.strip_prefix(&root).ok()?;
    let rel_str = rel.to_string_lossy().replace('\\', "/");
    if let Some(stripped) = rel_str.strip_prefix("conversation-media/") {
        return Some(stripped.to_string());
    }
    if rel_str.starts_with(GENERATED_MEDIA_PREFIX) || rel_str.starts_with(SESSION_SANDBOXES_PREFIX)
    {
        return Some(rel_str);
    }
    None
}

pub fn save_attachment_bytes(
    conversation_id: &str,
    _attachment_id: &str,
    bytes: &[u8],
    file_name: &str,
) -> Result<String> {
    if conversation_id.trim().is_empty() {
        anyhow::bail!("conversation_id required");
    }
    let (dir, rel_base) = sandbox_attachment_rel(conversation_id)?;
    let (id, file_path, mut file) = create_sandbox_attachment_file(&dir, file_name)?;
    if let Err(e) = file.write_all(bytes) {
        let _ = fs::remove_file(&file_path);
        return Err(e).with_context(|| format!("write attachment file {}", file_path.display()));
    }
    let file_name = file_path
        .file_name()
        .and_then(|name| name.to_str())
        .context("sandbox attachment filename is not UTF-8")?;
    let rel = format!("{rel_base}/{file_name}");
    log::info!(
        "save_attachment_bytes conv={conversation_id} id={id} -> {} rel={} ({} bytes)",
        file_path.display(),
        rel,
        bytes.len()
    );
    Ok(rel)
}

/// Copy a local filesystem file into the conversation sandbox (no base64 / full-buffer IPC).
///
/// Used by desktop Composer when the user picks or drops a path. Videos are rejected —
/// they must go through the OSS upload path.
pub fn save_attachment_from_path(
    conversation_id: &str,
    source_path: &Path,
    file_name: &str,
) -> Result<String> {
    if conversation_id.trim().is_empty() {
        anyhow::bail!("conversation_id required");
    }
    let source = super::access::normalize_user_path(&source_path.to_string_lossy())
        .with_context(|| format!("normalize attachment source {}", source_path.display()))?;
    if !source.is_file() {
        anyhow::bail!(crate::i18n::tf(
            "err.file_missing",
            crate::i18n::current_ui_locale(),
            &[("path", &source.display().to_string())],
        ));
    }
    let display_name = {
        let trimmed = file_name.trim();
        if !trimmed.is_empty() {
            trimmed.to_string()
        } else {
            source
                .file_name()
                .and_then(|n| n.to_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("attachment")
                .to_string()
        }
    };
    if super::video::is_video_file_name(&display_name) {
        anyhow::bail!(crate::i18n::t(
            "err.video_sandbox_forbidden",
            crate::i18n::current_ui_locale(),
        ));
    }
    let meta = fs::metadata(&source)
        .with_context(|| format!("stat attachment source {}", source.display()))?;
    ensure_composer_attachment_size(meta.len(), &display_name)?;

    let (dir, rel_base) = sandbox_attachment_rel(conversation_id)?;
    let (id, dest_path, mut dest) = create_sandbox_attachment_file(&dir, &display_name)?;
    let copy_result = (|| -> Result<u64> {
        let mut src = fs::File::open(&source)
            .with_context(|| format!("open attachment source {}", source.display()))?;
        std::io::copy(&mut src, &mut dest)
            .with_context(|| format!("copy attachment to {}", dest_path.display()))
    })();
    if let Err(e) = copy_result {
        let _ = fs::remove_file(&dest_path);
        return Err(e);
    }
    let stored_name = dest_path
        .file_name()
        .and_then(|name| name.to_str())
        .context("sandbox attachment filename is not UTF-8")?;
    let rel = format!("{rel_base}/{stored_name}");
    log::info!(
        "save_attachment_from_path conv={conversation_id} id={id} src={} -> {} rel={} ({} bytes)",
        source.display(),
        dest_path.display(),
        rel,
        meta.len()
    );
    Ok(rel)
}

pub fn read_media_bytes(storage_rel_path: &str) -> Result<Vec<u8>> {
    let path = match media_abs_path(storage_rel_path) {
        Ok(p) => p,
        Err(e) if e.to_string().contains("media access denied") => {
            log::info!(
                "read_media_bytes: session access denied for {storage_rel_path}, trying unscoped"
            );
            media_abs_path_unscoped(storage_rel_path)?
        }
        Err(e) => return Err(e),
    };
    fs::read(&path).with_context(|| format!("read media file {}", path.display()))
}

/// Absolute path, MIME, and file name for HTTP download/stream handlers (storage rel).
pub fn chat_media_file_meta(storage_rel_path: &str) -> Result<(PathBuf, String, String)> {
    let path = media_abs_path(storage_rel_path)?;
    media_file_meta_from_path(&path)
}

/// Absolute path, MIME, and file name for HTTP handlers resolving arbitrary media refs.
pub fn chat_media_ref_file_meta(media_ref: &str) -> Result<(PathBuf, String, String)> {
    let trimmed = media_ref.trim();
    if trimmed.is_empty() {
        anyhow::bail!("empty media ref");
    }
    let rel = trimmed
        .strip_prefix(MEDIA_URI_SCHEME)
        .map(str::trim)
        .unwrap_or(trimmed);
    let path = resolve_local_media_path(rel)?;
    if !path.is_file() {
        anyhow::bail!("media file not found: {}", path.display());
    }
    media_file_meta_from_path(&path)
}

fn media_file_meta_from_path(path: &Path) -> Result<(PathBuf, String, String)> {
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("attachment")
        .to_string();
    let mime_type = mime_from_path(path);
    Ok((path.to_path_buf(), mime_type, file_name))
}

/// `{conv}/{id}.bin` -> `{conv}/{id}.wav` when the wav file already exists on disk.
pub fn stored_wav_sibling_rel(storage_rel_path: &str) -> Option<String> {
    let rel = storage_rel_path.trim().trim_start_matches('/');
    let lower = rel.to_ascii_lowercase();
    if !lower.ends_with(".bin") {
        return None;
    }
    let wav_rel = format!("{}wav", &rel[..rel.len() - 3]);
    let path = match media_abs_path(&wav_rel) {
        Ok(p) => p,
        Err(_) => return None,
    };
    if path.is_file() {
        Some(wav_rel)
    } else {
        None
    }
}

/// Parse storage rel into `(conversation_segment, attachment_id_stem)`.
/// Supports legacy `{conv}/{file}` and `{user}/{conv}/{file}` layouts.
pub fn parse_conversation_media_ids(storage_rel_path: &str) -> Option<(String, String)> {
    let parsed = parse_storage_rel(storage_rel_path).ok()?;
    let conv = parsed.conversation_segment;
    let stem = parsed
        .file_name
        .rsplit_once('.')
        .map(|(s, _)| s)
        .unwrap_or(parsed.file_name.as_str());
    if conv.is_empty() || stem.is_empty() {
        return None;
    }
    Some((conv, stem.to_string()))
}

pub fn conversation_media_abs_to_rel(path: &Path) -> Option<String> {
    let root = match conversation_media_root() {
        Ok(r) => r,
        Err(_) => return None,
    };
    let rel = match path.strip_prefix(&root) {
        Ok(r) => r,
        Err(_) => return None,
    };
    Some(rel.to_string_lossy().replace('\\', "/"))
}

fn wav_sibling_abs_path(path: &Path) -> Option<PathBuf> {
    let name = path.file_name().and_then(|n| n.to_str())?;
    let lower = name.to_ascii_lowercase();
    if !lower.ends_with(".bin") {
        return None;
    }
    let wav_name = format!("{}wav", &name[..name.len() - 3]);
    let wav_path = path.with_file_name(wav_name);
    if wav_path.is_file() {
        Some(wav_path)
    } else {
        None
    }
}

pub fn read_chat_media_preview(storage_rel_path: &str) -> Result<ChatMediaPreview> {
    let path = media_abs_path(storage_rel_path)?;
    read_file_preview(&path)
}

/// Preview a media reference from assistant `MEDIA:` markers or attachment metadata.
pub fn read_media_ref_preview(media_ref: &str) -> Result<ChatMediaPreview> {
    let trimmed = media_ref.trim();
    if trimmed.is_empty() {
        anyhow::bail!("empty media ref");
    }

    let rel = trimmed
        .strip_prefix(MEDIA_URI_SCHEME)
        .map(str::trim)
        .unwrap_or(trimmed);

    if path_has_traversal(rel) {
        anyhow::bail!("media path traversal not allowed: {rel}");
    }

    let path = resolve_local_media_path(rel)?;
    assert_app_media_preview_allowed(&path)?;
    read_file_preview(&path)
}

fn read_file_preview(path: &Path) -> Result<ChatMediaPreview> {
    assert_app_media_preview_allowed(path)?;
    if let Some(wav_path) = wav_sibling_abs_path(path) {
        let bytes = fs::read(&wav_path)
            .with_context(|| format!("read persisted wav {}", wav_path.display()))?;
        let file_name = wav_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("audio.wav")
            .to_string();
        return Ok(ChatMediaPreview {
            data_base64: base64::engine::general_purpose::STANDARD.encode(&bytes),
            mime_type: "audio/wav".into(),
            file_name,
        });
    }
    let bytes = fs::read(path).with_context(|| format!("read media file {}", path.display()))?;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("attachment")
        .to_string();
    let mime_type = mime_from_path(path);
    let mime_lower = mime_type.to_ascii_lowercase();
    let needs_audio_transcode = mime_lower.starts_with("audio/") && mime_lower != "audio/wav"
        || file_name.to_ascii_lowercase().ends_with(".bin");
    if needs_audio_transcode && crate::media::ffmpeg::ffmpeg_available() {
        let storage_ctx = conversation_media_abs_to_rel(path).map(|rel| {
            let (conv, id) =
                parse_conversation_media_ids(&rel).unwrap_or((String::new(), String::new()));
            crate::media::audio::AudioStorageContext {
                storage_rel_path: Some(rel),
                conversation_id: conv,
                attachment_id: id,
            }
        });
        match crate::media::audio::prepare_audio_bytes_for_asr_cached(
            &bytes,
            &mime_type,
            &file_name,
            storage_ctx.as_ref(),
        ) {
            Ok(prepared) if prepared.mime_type == "audio/wav" => {
                return Ok(ChatMediaPreview {
                    data_base64: base64::engine::general_purpose::STANDARD.encode(&prepared.bytes),
                    mime_type: prepared.mime_type,
                    file_name: prepared.file_name,
                });
            }
            Err(e) => {
                log::warn!(
                    "media preview: audio transcode failed for {}: {:#}",
                    file_name,
                    e
                );
            }
            _ => {}
        }
    }
    Ok(ChatMediaPreview {
        data_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
        mime_type,
        file_name,
    })
}

fn mime_from_path(path: &Path) -> String {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png".into(),
        Some("jpg") | Some("jpeg") => "image/jpeg".into(),
        Some("gif") => "image/gif".into(),
        Some("webp") => "image/webp".into(),
        Some("pdf") => "application/pdf".into(),
        Some("txt") => "text/plain".into(),
        Some("md") => "text/markdown".into(),
        Some("json") => "application/json".into(),
        Some("mp4") | Some("m4v") => "video/mp4".into(),
        Some("webm") => "video/webm".into(),
        Some("mov") => "video/quicktime".into(),
        Some("mkv") => "video/x-matroska".into(),
        Some("mp3") => "audio/mpeg".into(),
        Some("wav") => "audio/wav".into(),
        Some("m4a") => "audio/mp4".into(),
        Some("aac") => "audio/aac".into(),
        Some("ogg") => "audio/ogg".into(),
        Some("flac") => "audio/flac".into(),
        _ => "application/octet-stream".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sandbox_attachment_filename_contains_twelve_hex_id() {
        let dir = std::env::temp_dir().join(format!("pointer-store-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();

        let (id, path, mut file) =
            create_sandbox_attachment_file(&dir, "Quarterly report.pdf").unwrap();
        file.write_all(b"test").unwrap();
        let rel = format!(
            "session-sandboxes/user-1/attachments/{}",
            path.file_name().unwrap().to_string_lossy()
        );

        assert_eq!(id.len(), SHORT_ATTACHMENT_ID_LEN);
        assert!(id.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(
            short_attachment_id_from_sandbox_rel(&rel).as_deref(),
            Some(id.as_str())
        );
        assert!(path.is_file());

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn short_id_parser_rejects_non_attachment_names() {
        assert!(
            short_attachment_id_from_sandbox_rel("session-sandboxes/user-1/notes/report.pdf")
                .is_none()
        );
        assert!(short_attachment_id_from_sandbox_rel(
            "session-sandboxes/user-1/attachments/abcdef_notes.pdf"
        )
        .is_none());
    }

    #[test]
    fn save_attachment_from_path_copies_without_loading_all_bytes_in_caller() {
        let _lock = crate::storage::test_app_data_dir_lock();
        let data = tempfile::tempdir().expect("app data");
        crate::storage::set_test_app_data_dir(data.path().to_path_buf());

        let src_dir = tempfile::tempdir().expect("src");
        let src = src_dir.path().join("photo.png");
        let payload = {
            let mut bytes = vec![0x89u8, 0x50, 0x4E, 0x47];
            bytes.extend(vec![7u8; 64_000]);
            bytes
        };
        fs::write(&src, &payload).expect("write src");

        let rel = save_attachment_from_path("conv-path-copy", &src, "photo.png").expect("copy");
        assert!(rel.contains("session-sandboxes/"));
        assert!(rel.contains("attachments/"));
        assert!(rel.ends_with("_photo.png") || rel.contains("_photo.png"));

        let dest = media_abs_path(&rel).expect("resolve");
        assert_eq!(fs::read(&dest).expect("read dest"), payload);

        let _ = fs::remove_dir_all(data.path());
    }
}
