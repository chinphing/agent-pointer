use anyhow::{Context, Result};
use base64::Engine;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::models::ChatMediaPreview;
use crate::session_sandbox::ANONYMOUS_SEGMENT;
use crate::storage::app_data_dir;
use crate::user_storage::session_user_id_for_conversation;

use super::access::{assert_app_media_preview_allowed, path_has_traversal};
use super::filename::safe_attachment_basename;
use super::layout::{
    parse_storage_rel, resolve_v2_host_path, verify_storage_rel_access, LAYOUT_VERSION_PREFIX,
};
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
        anyhow::bail!("文件超过 {} MB 上限", (limit / (1024 * 1024)).max(1));
    }
    Ok(())
}

/// True for rel paths stored directly under `{app_data}/` (not conversation attachment layout).
pub fn is_app_data_subtree_rel(raw: &str) -> bool {
    let rel = raw.trim().trim_start_matches('/');
    // v2 keys live on the user data volume, not under `{app_data}/v2`.
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
    let dir = crate::user_data_paths::attachments_host_dir(&session_user_id, conversation_id)?;
    let conv = crate::storage::sanitize_storage_dir_segment(conversation_id.trim());
    let stored = crate::user_data_paths::UserRelPath::new(
        crate::user_data_paths::UserRootKind::Data,
        format!("attachments/{conv}/file"),
    )?;
    let encoded = stored.encode();
    let rel_base = encoded
        .rsplit_once('/')
        .map(|(base, _)| base.to_string())
        .unwrap_or(encoded);
    log::info!(
        "attachment dir conv={conversation_id} user={} rel_base={rel_base}",
        session_user_id
    );
    Ok((dir, rel_base))
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

fn conversation_segment_from_user_rel(rel: &crate::user_data_paths::UserRelPath) -> String {
    let parts: Vec<&str> = rel
        .rel
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect();
    match parts.as_slice() {
        ["attachments", conv, ..]
        | ["media", "chat", conv, ..]
        | ["media", "generated", conv, ..]
        | ["turn-baselines", conv, ..] => conv.to_string(),
        ["captures", _, conv, ..] => conv.to_string(),
        _ => String::new(),
    }
}

/// Host file for a stored `home:` / `data:` path. The user comes from the
/// bound session, otherwise from the conversation id inside the relative segment.
fn resolve_user_rel_host(stored: &crate::user_data_paths::UserRelPath) -> Result<PathBuf> {
    let (home, data) = if let Some(session) = crate::env::current_session() {
        (
            session.scope().home_host.clone(),
            session.scope().data_host.clone(),
        )
    } else {
        let conv = conversation_segment_from_user_rel(stored);
        let uid = if conv.is_empty() {
            crate::session_user_env::current_session_user_id().unwrap_or_default()
        } else {
            session_user_id_for_conversation(&conv)
        };
        if uid.is_empty() && conv.is_empty() {
            log::warn!("media: user relative path has no session user");
            anyhow::bail!("媒体路径缺少当前用户");
        }
        let users = crate::user_data_paths::users_root()?;
        (
            crate::user_data_paths::user_home_host(&users, &uid, &conv)?,
            crate::user_data_paths::user_data_host(&users, &uid, &conv)?,
        )
    };
    let root = match stored.root {
        crate::user_data_paths::UserRootKind::Home => &home,
        crate::user_data_paths::UserRootKind::Data => &data,
    };
    let path = stored.join_host(&home, &data);
    if !path.starts_with(root) {
        log::warn!(
            "media: user relative path escaped {} -> {}",
            root.display(),
            path.display()
        );
        anyhow::bail!("媒体文件不在当前用户目录内");
    }
    if path.is_file() {
        let canonical = path
            .canonicalize()
            .with_context(|| format!("canonicalize {}", path.display()))?;
        let root_canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        if !canonical.starts_with(&root_canonical) {
            log::warn!(
                "media: canonical path left user tree {}",
                canonical.display()
            );
            anyhow::bail!("媒体文件不在当前用户目录内");
        }
        return Ok(canonical);
    }
    log::warn!("media: user file missing {}", path.display());
    anyhow::bail!("media file not found under user data: {}", path.display())
}

/// Path the model should see for a stored key or an on-disk file.
///
/// Stored keys join the session root. A path already inside the session is
/// returned unchanged. A host path is translated into the session. Anything
/// that does not map is refused, so a host absolute path is not shown.
pub fn model_visible_path(conversation_id: &str, raw: &str) -> Result<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        anyhow::bail!("媒体路径为空");
    }
    let env = crate::env::current_env()
        .or_else(|| crate::env::session_env::env_for_conversation(conversation_id))
        .context("媒体路径缺少当前用户的执行环境")?;
    if env.is_denied() || env.path_map().is_host_passthrough() {
        log::warn!(
            "media: model path refused passthrough conversation={}",
            conversation_id.trim()
        );
        anyhow::bail!("缺少当前用户的执行环境");
    }
    if let Ok(stored) = crate::user_data_paths::UserRelPath::decode(raw) {
        let root = match stored.root {
            crate::user_data_paths::UserRootKind::Home => env.home_root(),
            crate::user_data_paths::UserRootKind::Data => env.data_root(),
        };
        if root.as_os_str().is_empty() {
            log::warn!(
                "media: model path missing session root conversation={}",
                conversation_id.trim()
            );
            anyhow::bail!("会话环境缺少目录根");
        }
        let path = root.join(&stored.rel);
        log::info!(
            "media: model path conversation={} path={}",
            conversation_id.trim(),
            path.display()
        );
        return Ok(path.display().to_string());
    }
    let path = Path::new(raw);
    if !path.is_absolute() {
        return Ok(raw.to_string());
    }
    if env.path_map().contains_guest(path) {
        return Ok(raw.to_string());
    }
    match env.path_map().to_agent(path) {
        Ok(guest) => {
            log::info!(
                "media: model path mapped conversation={} path={}",
                conversation_id.trim(),
                guest.display()
            );
            Ok(guest.display().to_string())
        }
        Err(error) => {
            log::warn!(
                "media: model path refused unmapped host path conversation={}: {error:#}",
                conversation_id.trim()
            );
            Err(error)
        }
    }
}

/// Path the model should see: the current session root plus the relative segment.
pub fn session_display_path(storage_rel_path: &str) -> Result<String> {
    if crate::user_data_paths::is_host_absolute(storage_rel_path) {
        log::warn!("media: model path refused host absolute path");
        anyhow::bail!("宿主绝对路径不能打开");
    }
    let session = crate::env::current_session().context("媒体路径缺少当前用户的执行环境")?;
    if let Ok(stored) = crate::user_data_paths::UserRelPath::decode(storage_rel_path) {
        let root = match stored.root {
            crate::user_data_paths::UserRootKind::Home => session.home_root(),
            crate::user_data_paths::UserRootKind::Data => session.data_root(),
        };
        return Ok(root.join(&stored.rel).display().to_string());
    }
    let host = media_abs_path(storage_rel_path)?;
    session
        .path_map()
        .to_agent(&host)
        .map(|path| path.display().to_string())
}

pub fn conversation_media_root() -> Result<PathBuf> {
    Ok(app_data_dir()?.join(CONVERSATION_MEDIA_DIR))
}

pub fn media_abs_path(storage_rel_path: &str) -> Result<PathBuf> {
    if crate::user_data_paths::is_host_absolute(storage_rel_path) {
        log::warn!("media: refuse host absolute path {}", storage_rel_path);
        anyhow::bail!("宿主绝对路径不能打开");
    }
    if let Ok(stored) = crate::user_data_paths::UserRelPath::decode(storage_rel_path) {
        return resolve_user_rel_host(&stored);
    }
    let rel = storage_rel_path.trim().trim_start_matches('/');
    if rel.starts_with(&format!("{LAYOUT_VERSION_PREFIX}/")) {
        if let Err(e) = verify_storage_rel_access(storage_rel_path) {
            log::info!("media_abs_path verify_storage_rel_access FAILED: {e:#}");
            return Err(e);
        }
        let path = resolve_v2_host_path(storage_rel_path)?;
        if path.exists() {
            return Ok(path);
        }
        anyhow::bail!("media file not found under user data: {}", path.display());
    }
    if is_app_data_subtree_rel(storage_rel_path) {
        let path = app_data_dir()?.join(rel);
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
    if crate::user_data_paths::is_host_absolute(storage_rel_path) {
        log::warn!("media: refuse host absolute path {}", storage_rel_path);
        anyhow::bail!("宿主绝对路径不能打开");
    }
    if let Ok(stored) = crate::user_data_paths::UserRelPath::decode(storage_rel_path) {
        return resolve_user_rel_host(&stored);
    }
    let rel = storage_rel_path.trim().trim_start_matches('/');
    if rel.is_empty() || rel.contains("..") {
        anyhow::bail!("invalid media rel path");
    }
    if rel.starts_with(&format!("{LAYOUT_VERSION_PREFIX}/")) {
        let path = resolve_v2_host_path(storage_rel_path)?;
        if path.is_file() {
            return Ok(path);
        }
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
        anyhow::bail!("文件不存在: {}", source.display());
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
        anyhow::bail!("视频请通过 OSS 上传，勿直接复制到沙箱");
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
    read_chat_media_preview_in_env(storage_rel_path, None)
}

/// 聊天附件预览。
///
/// 沙箱环境且路径是 v2（用户 data 卷，guest `/data/...`）时，用该环境的读文件接口
/// 取字节再交给浏览器。本地档位、没有会话环境、以及仍在应用数据目录里的旧路径，
/// 继续读本机文件。
pub fn read_chat_media_preview_in_env(
    storage_rel_path: &str,
    env: Option<std::sync::Arc<dyn crate::env::Environment>>,
) -> Result<ChatMediaPreview> {
    let sandbox = env.as_ref().filter(|env| is_sandbox_data_env(env.as_ref()));
    if let Some(env) = sandbox {
        if let Ok(guest) = v2_guest_data_path(storage_rel_path) {
            let path = wav_sibling_in_env(env.as_ref(), &guest);
            log::info!(
                "chat media preview via environment rel={storage_rel_path} guest={}",
                path.display()
            );
            let body = crate::workspace_read::media_body_for_streaming(
                env.as_ref(),
                &path,
                MEDIA_REF_ENV_PREVIEW_MAX_BYTES,
            )
            .with_context(|| format!("read chat media {storage_rel_path} via environment"))?;
            let bytes = read_media_body(body)?;
            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .filter(|n| !n.is_empty())
                .unwrap_or("attachment")
                .to_string();
            let mime_type = mime_from_path(&path);
            return finish_media_preview(bytes, &file_name, &mime_type, None, env);
        }
        log::warn!(
            "chat media preview: sandbox refuses host fallback for non-v2 path rel={storage_rel_path}"
        );
        anyhow::bail!("沙箱媒体只支持用户 data 卷路径，拒绝回落宿主");
    }
    let path = media_abs_path(storage_rel_path)?;
    read_file_preview(&path)
}

/// 聊天附件下载 / 流式播放的字节来源。规则同 [`read_chat_media_preview_in_env`]。
pub fn chat_media_body_in_env(
    storage_rel_path: &str,
    env: Option<&dyn crate::env::Environment>,
    max_bytes: u64,
) -> Result<(crate::workspace_read::WorkspaceMediaBody, String, String)> {
    if let Some(env) = env {
        if is_sandbox_data_env(env) {
            match v2_guest_data_path(storage_rel_path) {
                Ok(guest) => {
                    log::info!(
                        "chat media body via environment rel={storage_rel_path} guest={}",
                        guest.display()
                    );
                    let body =
                        crate::workspace_read::media_body_for_streaming(env, &guest, max_bytes)
                            .with_context(|| {
                                format!("read chat media {storage_rel_path} via environment")
                            })?;
                    let file_name = guest
                        .file_name()
                        .and_then(|n| n.to_str())
                        .filter(|n| !n.is_empty())
                        .unwrap_or("attachment")
                        .to_string();
                    let mime_type = mime_from_path(&guest);
                    return Ok((body, mime_type, file_name));
                }
                Err(error) => {
                    log::warn!(
                        "chat media body: sandbox refuses host fallback rel={storage_rel_path}: {error:#}"
                    );
                    anyhow::bail!("沙箱媒体只支持用户 data 卷路径，拒绝回落宿主");
                }
            }
        }
    }
    let (path, mime_type, file_name) = chat_media_file_meta(storage_rel_path)?;
    Ok((
        crate::workspace_read::WorkspaceMediaBody::HostPath(path),
        mime_type,
        file_name,
    ))
}

/// Docker / Remote：guest data 根是 `/data`。LocalEnv::for_user 的 data 是宿主路径，不算沙箱。
fn is_sandbox_data_env(env: &dyn crate::env::Environment) -> bool {
    !env.is_denied()
        && !env.path_map().is_host_passthrough()
        && env.data_root() == Path::new(crate::env::mounts::GUEST_DATA)
}

/// v2 `storage_rel_path` → guest `/data/{kind}/{conversation}/{file}`.
fn v2_guest_data_path(storage_rel_path: &str) -> Result<PathBuf> {
    let parsed = parse_storage_rel(storage_rel_path)?;
    if !parsed.versioned {
        anyhow::bail!("not a v2 data-volume path");
    }
    let kind = parsed.kind.as_deref().unwrap_or("attachments");
    let mut path = PathBuf::from(crate::env::mounts::GUEST_DATA);
    for segment in kind.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            anyhow::bail!("invalid v2 data kind");
        }
        path.push(segment);
    }
    if parsed.conversation_segment.is_empty()
        || parsed.conversation_segment == "."
        || parsed.conversation_segment == ".."
        || parsed.file_name.is_empty()
        || parsed.file_name.contains('/')
        || parsed.file_name == "."
        || parsed.file_name == ".."
    {
        anyhow::bail!("invalid v2 data path");
    }
    path.push(&parsed.conversation_segment);
    path.push(&parsed.file_name);
    Ok(path)
}

fn wav_sibling_in_env(env: &dyn crate::env::Environment, path: &Path) -> PathBuf {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return path.to_path_buf();
    };
    let lower = name.to_ascii_lowercase();
    if !lower.ends_with(".bin") {
        return path.to_path_buf();
    }
    let wav_name = format!("{}wav", &name[..name.len() - 3]);
    let wav_path = path.with_file_name(wav_name);
    match env.metadata(&wav_path) {
        Ok(meta) if meta.is_file => wav_path,
        Ok(_) => path.to_path_buf(),
        Err(_) => path.to_path_buf(),
    }
}

fn read_media_body(body: crate::workspace_read::WorkspaceMediaBody) -> Result<Vec<u8>> {
    match body {
        crate::workspace_read::WorkspaceMediaBody::HostPath(host) => {
            fs::read(&host).with_context(|| format!("read media file {}", host.display()))
        }
        crate::workspace_read::WorkspaceMediaBody::Inline(bytes) => Ok(bytes),
    }
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

/// 沙箱档位下媒体引用预览的整段读上限（32 MB，与工作区在线预览同一口径）。
pub const MEDIA_REF_ENV_PREVIEW_MAX_BYTES: u64 = 32 * 1024 * 1024;

/// 沙箱档位下媒体引用**下载**的整段读上限（256 MB）。
///
/// 远端后端（envd）宿主上没有文件句柄，只能整段读进内存；超过上限明确报错，
/// 不静默截断。容器后端有宿主挂载，走宿主流式，不受这个上限影响。
pub const MEDIA_REF_ENV_DOWNLOAD_MAX_BYTES: u64 = 256 * 1024 * 1024;

/// 媒体引用在**会话环境内**的路径。
///
/// 只有绝对路径才按会话环境解析：相对 / 存储 rel 路径是应用数据目录里的附件，
/// 宿主解析失败就是真的没有，不该拿去环境里猜。
pub fn media_ref_env_path(media_ref: &str) -> Result<PathBuf> {
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
    let path = PathBuf::from(rel);
    if !path.is_absolute() {
        anyhow::bail!("media ref is not an absolute path: {rel}");
    }
    Ok(path)
}

/// 媒体引用的字节来源：宿主上找不到时按**会话环境**再解析一次。
///
/// 沙箱档位下 `MEDIA:` 引用是**环境内路径**（`/workspace/x.png`），宿主上没有这个
/// 文件，只有取到该会话的环境才读得到。`env = None` 时直接返回宿主错误——这条
/// 路由的调用方可能来自非工作区场景，缺会话参数**不 fail-closed**，维持宿主读取。
///
/// 字节来源判定复用 [`crate::workspace_read::media_body_for_streaming`]：
/// 宿主挂载（local / docker）→ 宿主流式；远端后端 → 整段读（有上限）。
pub fn media_ref_body_in_env(
    media_ref: &str,
    env: Option<&dyn crate::env::Environment>,
    max_bytes: u64,
) -> Result<(crate::workspace_read::WorkspaceMediaBody, String, String)> {
    match chat_media_ref_file_meta(media_ref) {
        Ok((path, mime_type, file_name)) => Ok((
            crate::workspace_read::WorkspaceMediaBody::HostPath(path),
            mime_type,
            file_name,
        )),
        Err(host_err) => {
            let Some(env) = env else {
                return Err(host_err);
            };
            // 相对 / 存储 rel 路径只在宿主应用数据目录里有意义：环境里没得猜，
            // 保持宿主错误原文（缺会话参数时也走这里，行为与以前一致）。
            let Ok(path) = media_ref_env_path(media_ref) else {
                return Err(host_err);
            };
            let body = crate::workspace_read::media_body_for_streaming(env, &path, max_bytes)
                .with_context(|| format!("read media ref {media_ref} from session env"))?;
            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .filter(|n| !n.is_empty())
                .unwrap_or("attachment")
                .to_string();
            let mime_type = mime_from_path(&path);
            Ok((body, mime_type, file_name))
        }
    }
}

/// 同 [`read_media_ref_preview`]，宿主上找不到时按**会话环境**再读一次。
///
/// `env = None` 时行为与 [`read_media_ref_preview`] 完全一致（宿主读取），
/// 不因为缺会话参数而失败。
pub fn read_media_ref_preview_in_env(
    media_ref: &str,
    env: Option<&dyn crate::env::Environment>,
) -> Result<ChatMediaPreview> {
    match read_media_ref_preview(media_ref) {
        Ok(preview) => Ok(preview),
        Err(host_err) => {
            let Some(env) = env else {
                return Err(host_err);
            };
            // 相对 / 存储 rel 路径只在宿主应用数据目录里有意义：环境里没得猜，
            // 保持宿主错误原文（缺会话参数时也走这里，行为与以前一致）。
            if media_ref_env_path(media_ref).is_err() {
                return Err(host_err);
            }
            let (body, mime_type, file_name) =
                media_ref_body_in_env(media_ref, Some(env), MEDIA_REF_ENV_PREVIEW_MAX_BYTES)?;
            let bytes = match body {
                crate::workspace_read::WorkspaceMediaBody::HostPath(host) => fs::read(&host)
                    .with_context(|| format!("read media file {}", host.display()))?,
                crate::workspace_read::WorkspaceMediaBody::Inline(bytes) => bytes,
            };
            Ok(ChatMediaPreview {
                data_base64: base64::engine::general_purpose::STANDARD.encode(&bytes),
                mime_type,
                file_name,
            })
        }
    }
}

fn ensure_preview_stays_in_session(path: &Path) -> Result<()> {
    let Some(session) = crate::env::current_session() else {
        return Ok(());
    };
    crate::media::media_ref::session_host_file(session.environment().as_ref(), path).map(|_| ())
}

fn read_file_preview(path: &Path) -> Result<ChatMediaPreview> {
    assert_app_media_preview_allowed(path)?;
    ensure_preview_stays_in_session(path)?;
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
    let env = crate::env::current_session()
        .map(|session| session.environment())
        .unwrap_or_else(crate::env::process_io);
    finish_media_preview(bytes, &file_name, &mime_type, Some(path), &env)
}

fn finish_media_preview(
    bytes: Vec<u8>,
    file_name: &str,
    mime_type: &str,
    storage_abs: Option<&Path>,
    env: &std::sync::Arc<dyn crate::env::Environment>,
) -> Result<ChatMediaPreview> {
    let mime_lower = mime_type.to_ascii_lowercase();
    let needs_audio_transcode = mime_lower.starts_with("audio/") && mime_lower != "audio/wav"
        || file_name.to_ascii_lowercase().ends_with(".bin");
    if needs_audio_transcode && crate::media::ffmpeg::ffmpeg_available(env) {
        let storage_ctx = storage_abs
            .and_then(conversation_media_abs_to_rel)
            .map(|rel| {
                let (conv, id) =
                    parse_conversation_media_ids(&rel).unwrap_or((String::new(), String::new()));
                crate::media::audio::AudioStorageContext {
                    storage_rel_path: Some(rel),
                    conversation_id: conv,
                    attachment_id: id,
                }
            });
        match crate::media::audio::prepare_audio_bytes_for_asr_cached(
            env,
            &bytes,
            mime_type,
            file_name,
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
        mime_type: mime_type.to_string(),
        file_name: file_name.to_string(),
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
        assert!(
            rel.starts_with("data:attachments/"),
            "expected user data rel, got {rel}"
        );
        assert!(rel.contains("attachments/"));
        assert!(rel.ends_with("_photo.png") || rel.contains("_photo.png"));

        let dest = media_abs_path(&rel).expect("resolve");
        assert_eq!(fs::read(&dest).expect("read dest"), payload);

        let _ = fs::remove_dir_all(data.path());
    }

    #[test]
    fn v2_attachment_is_not_resolved_under_app_data_v2_prefix() {
        let _lock = crate::storage::test_app_data_dir_lock();
        let data = tempfile::tempdir().expect("app data");
        crate::storage::set_test_app_data_dir(data.path().to_path_buf());
        let rel = save_attachment_bytes("conv-v2-read", "ignored", b"hello-v2", "note.txt")
            .expect("save");
        assert!(rel.starts_with("data:attachments/"), "{rel}");
        let path = crate::media::resolve::resolve_local_media_path(&rel).expect("resolve v2");
        assert_eq!(fs::read(&path).expect("read"), b"hello-v2");
        let mistaken = data.path().join(rel.trim_start_matches('/'));
        assert_ne!(path, mistaken);
        assert!(!mistaken.exists());
    }

    #[test]
    fn absolute_storage_rel_is_not_opened() {
        let tmp = tempfile::tempdir().expect("tmp");
        let secret = tmp.path().join("secret.txt");
        fs::write(&secret, b"host-secret").expect("write");
        let err = media_abs_path(&secret.display().to_string()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("宿主绝对路径不能打开"), "{msg}");
        assert!(!msg.contains("host-secret"), "{msg}");
    }
}
