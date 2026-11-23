//! Persist screen-capture bundles for the execution environment.
//!
//! New files are stored under the session data root as `captures/{date}/{conversation}/{file}`.
//! The value handed to the client is `data:captures/...`. Older
//! `{date}/{conversation}/{file}` keys still resolve. A host absolute path does not.
//!
//! Only writes assets the active tier uses — no extra JPEG/PNG generation on disk.

use crate::agents::computer::state::ScreenCaptureResult;
use crate::agents::computer::vision::{screen, screen_overlay};
use crate::media::jpeg_vision;
use crate::models::ComputerAnnotatedPreview;
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Day-level folders older than this are removed on app startup (desktop).
pub const CAPTURE_RETENTION_DAYS: i64 = 7;

/// On-disk capture dumps are opt-in: settings → 调试 → 「保存截图到本地」.
pub fn computer_capture_dump_enabled() -> bool {
    crate::platform_config::effective_settings_global().debug_save_computer_captures
}

fn sanitize_path_segment(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

struct CaptureStore {
    env: Arc<dyn crate::env::Environment>,
    root: PathBuf,
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn join_capture(root: &Path, rel: &str) -> PathBuf {
    let rel = rel
        .trim()
        .trim_start_matches(['/', '\\'])
        .replace('\\', "/");
    let text = path_text(root);
    if crate::workspace_resolve::looks_like_sandbox_guest_path(&text) {
        let root = text.trim_end_matches('/');
        if rel.is_empty() {
            return PathBuf::from(root);
        }
        return PathBuf::from(format!("{root}/{rel}"));
    }
    if rel.is_empty() {
        return root.to_path_buf();
    }
    root.join(rel)
}

fn capture_parent(path: &Path) -> Option<PathBuf> {
    let text = path_text(path);
    if crate::workspace_resolve::looks_like_sandbox_guest_path(&text) {
        let parent = text.trim_end_matches('/').rsplit_once('/')?.0;
        if parent.is_empty() {
            return None;
        }
        return Some(PathBuf::from(parent));
    }
    path.parent().map(Path::to_path_buf)
}

/// 截图键里的会话段。新键是 `data:captures/{日期}/{会话}/{文件}`，
/// 旧键是 `{日期}/{会话}/{文件}`。宿主绝对路径返回 `None`。
pub fn capture_conversation_id(rel: &str) -> Option<String> {
    let normalized = rel.trim().replace('\\', "/");
    if normalized.is_empty() {
        return None;
    }
    if let Ok(stored) = crate::user_data_paths::UserRelPath::decode(&normalized) {
        if stored.root != crate::user_data_paths::UserRootKind::Data {
            log::warn!("computer capture: refuse non-data key {normalized}");
            return None;
        }
        let text = stored.rel.to_string_lossy().replace('\\', "/");
        return conversation_from_capture_segments(&text, true);
    }
    if crate::user_data_paths::is_host_absolute(&normalized) {
        let stripped = normalized.trim_start_matches('/');
        if conversation_from_capture_segments(stripped, false).is_some() {
            return conversation_from_capture_segments(stripped, false);
        }
        log::warn!("computer capture: refuse host absolute path");
        return None;
    }
    conversation_from_capture_segments(&normalized, false)
}

fn conversation_from_capture_segments(rel: &str, under_captures: bool) -> Option<String> {
    let segments: Vec<&str> = rel
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .collect();
    let segments = if under_captures {
        if segments.first().copied() != Some("captures") {
            return None;
        }
        &segments[1..]
    } else {
        segments.as_slice()
    };
    if segments.len() < 3 || segments.iter().any(|segment| *segment == "..") {
        return None;
    }
    let date = segments[0].as_bytes();
    if date.len() != 10 || date[4] != b'-' || date[7] != b'-' {
        return None;
    }
    if !date
        .iter()
        .enumerate()
        .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit())
    {
        return None;
    }
    let conversation_id = segments[1].trim();
    (!conversation_id.is_empty()).then(|| conversation_id.to_string())
}

fn stored_capture_key(date: &str, conversation: &str, file: &str) -> Option<String> {
    match crate::user_data_paths::UserRelPath::new(
        crate::user_data_paths::UserRootKind::Data,
        format!("captures/{date}/{conversation}/{file}"),
    ) {
        Ok(path) => {
            let encoded = path.encode();
            log::info!("computer capture: stored={encoded}");
            Some(encoded)
        }
        Err(error) => {
            log::warn!(
                "computer capture: refuse key date={date} conversation={conversation} file={file}: {error:#}"
            );
            None
        }
    }
}

fn capture_data_rel(rel: &str) -> Result<(String, String)> {
    if crate::user_data_paths::is_host_absolute(rel.trim())
        && capture_conversation_id(rel).is_none()
    {
        log::warn!("computer capture: refuse host absolute path");
        bail!("宿主绝对路径不能打开");
    }
    let normalized = rel.trim().replace('\\', "/");
    if let Ok(stored) = crate::user_data_paths::UserRelPath::decode(&normalized) {
        let conversation_id =
            capture_conversation_id(&normalized).context("invalid or disallowed capture path")?;
        let data_rel = stored.rel.to_string_lossy().replace('\\', "/");
        return Ok((conversation_id, data_rel));
    }
    let conversation_id =
        capture_conversation_id(&normalized).context("invalid or disallowed capture path")?;
    let legacy = normalized.trim_start_matches('/');
    Ok((conversation_id, format!("captures/{legacy}")))
}

fn checked_capture_rel(root: &Path, rel: &str) -> Option<PathBuf> {
    let normalized = rel.trim().replace('\\', "/");
    if normalized.is_empty()
        || normalized.starts_with('/')
        || Path::new(&normalized).is_absolute()
        || normalized
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return None;
    }
    Some(join_capture(root, &normalized))
}

fn local_capture_store() -> Result<CaptureStore> {
    let root = crate::storage::app_data_dir()?.join("computer-captures");
    Ok(CaptureStore {
        env: crate::env::process_io(),
        root,
    })
}

fn conversation_workspace_is_guest(conversation_id: &str) -> Result<bool> {
    let store = crate::conversation_store::global_store().context("读取会话工作区失败")?;
    let workspace = store
        .workspace_root(conversation_id)
        .with_context(|| format!("读取会话工作区失败（conversation={conversation_id}）"))?;
    Ok(crate::workspace_resolve::looks_like_sandbox_guest_path(
        workspace.trim(),
    ))
}

/// 截图根跟着执行环境走。沙箱写 `/data/captures`，本地写应用数据目录。
fn capture_store(conversation_id: &str) -> Result<CaptureStore> {
    let id = conversation_id.trim();
    if id.is_empty() {
        bail!("缺少会话 id，无法保存电脑操控截图");
    }
    if let Some(env) = crate::env::session_env::env_for_conversation(id) {
        if env.is_denied() {
            bail!("沙箱会话的执行环境不可用，无法保存电脑操控截图（conversation={id}）");
        }
        if env.path_map().is_host_passthrough() || env.data_root().as_os_str().is_empty() {
            bail!("电脑操控截图缺少当前用户的执行环境（conversation={id}）");
        }
        let data = path_text(env.data_root());
        let data = data.trim_end_matches('/').to_string();
        if data.is_empty() {
            bail!("沙箱环境没有数据目录，无法保存电脑操控截图");
        }
        log::info!("computer capture: session environment conversation={id}");
        return Ok(CaptureStore {
            env,
            root: PathBuf::from(format!("{data}/captures")),
        });
    }
    match conversation_workspace_is_guest(id) {
        Ok(true) => {
            log::warn!(
                "computer capture: sandbox workspace has no session environment conversation={id}"
            );
            bail!("工作区在沙箱内，但当前没有会话环境，无法保存电脑操控截图（conversation={id}）");
        }
        Ok(false) => {
            bail!("电脑操控截图缺少当前用户的执行环境（conversation={id}）")
        }
        Err(error) => {
            log::warn!("computer capture: workspace lookup failed conversation={id}: {error:#}");
            bail!("无法确认会话工作区，已拒绝保存电脑操控截图（conversation={id}）");
        }
    }
}

fn capture_root_dir() -> Option<PathBuf> {
    match local_capture_store() {
        Ok(store) => Some(store.root),
        Err(error) => {
            log::warn!("computer capture: app_data_dir unavailable: {error:#}");
            None
        }
    }
}

/// Join `rel` under the local capture root; rejects `..` and absolute paths.
pub fn safe_capture_file_path(rel: &str) -> Option<PathBuf> {
    let root = capture_root_dir()?;
    checked_capture_rel(&root, rel)
}

/// Capture the host desktop for web manual preview: synthetic pointer + caret overlay, then
/// bandwidth-friendly JPEG (long edge ≤1280px, ≤100KB).
pub fn capture_manual_desktop_snapshot_jpeg() -> Result<Vec<u8>> {
    let packet = screen::screenshot_current_monitor()?;
    let marked = screen_overlay::mark_raw_jpeg_with_pointer_and_caret(
        &packet.jpeg,
        &packet.monitor,
        packet.global_pointer,
        packet.global_caret,
    )?;
    jpeg_vision::prepare_jpeg_for_manual_preview(&marked)
}

/// Read a saved annotated JPEG/PNG and return the same shape as live preview.
///
/// The relative path selects the conversation. A sandbox session is read from
/// `/data/captures` in that session's environment.
pub fn read_computer_capture_preview(rel: &str) -> anyhow::Result<ComputerAnnotatedPreview> {
    let (conversation_id, data_rel) = capture_data_rel(rel)?;
    let store = capture_store(&conversation_id)?;
    let path = checked_capture_rel(store.env.data_root(), &data_rel)
        .context("invalid or disallowed capture path")?;
    let bytes = store
        .env
        .read_file(&path)
        .with_context(|| format!("read {}", path.display()))?;
    Ok(ComputerAnnotatedPreview {
        image_base64: screen::encode_image_to_base64(&bytes),
        image_mime: screen::image_data_url_mime(&bytes).to_string(),
        caption: "本圈截图画面".into(),
    })
}

fn purge_in_store(store: &CaptureStore, days: i64) -> Result<usize> {
    if !store.env.exists(&store.root) {
        return Ok(0);
    }
    let cutoff = chrono::Local::now().date_naive() - chrono::Duration::days(days);
    let mut removed = 0usize;
    let entries = store
        .env
        .list_dir(&store.root)
        .with_context(|| format!("list capture root {}", store.root.display()))?;
    for entry in entries {
        if !entry.is_dir {
            continue;
        }
        if let Ok(date) = chrono::NaiveDate::parse_from_str(&entry.name, "%Y-%m-%d") {
            if date < cutoff {
                store
                    .env
                    .remove_dir_all(&entry.path)
                    .with_context(|| format!("remove capture day {}", entry.path.display()))?;
                removed += 1;
                log::info!("computer capture purge: removed {}", entry.path.display());
            }
        }
    }
    Ok(removed)
}

/// Deletes local `{app_data}/computer-captures/{YYYY-MM-DD}/` directories older than `days`.
///
/// Sandbox captures live in that session's `/data/captures` and are purged the
/// next time that session writes a capture.
pub fn purge_computer_captures_older_than_days(days: i64) -> std::io::Result<usize> {
    let store = match local_capture_store() {
        Ok(store) => store,
        Err(error) => {
            log::warn!("computer capture purge: could not resolve app data directory: {error:#}");
            return Ok(0);
        }
    };
    purge_in_store(&store, days).map_err(|error| {
        log::warn!("computer capture purge: {error:#}");
        std::io::Error::other(error.to_string())
    })
}

fn write_bytes(
    store: &CaptureStore,
    dir: &Path,
    pfx: &str,
    name: &str,
    ext: &str,
    ts: i64,
    bytes: &[u8],
) -> Option<String> {
    if bytes.is_empty() {
        return None;
    }
    let file_name = format!("{pfx}_{ts}_{name}.{ext}");
    let path = join_capture(dir, &file_name);
    match store.env.write_file(&path, bytes) {
        Ok(()) => Some(file_name),
        Err(error) => {
            log::warn!("computer capture dump: write {}: {error:#}", path.display());
            None
        }
    }
}

/// Map a vision slot label to an on-disk stem, e.g. `[Current screen]` → `current_screen`.
pub fn slot_label_to_disk_stem(slot_label: &str) -> String {
    let inner = slot_label
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim();
    sanitize_path_segment(&inner.to_ascii_lowercase().replace(' ', "_"))
}

/// Write one debug file under the conversation's capture directory.
///
/// `rel` is relative to `{date}/{conversation}/`. Sandbox sessions land in
/// `/data/captures/`; local sessions land in `{app_data}/computer-captures/`.
pub(crate) fn write_session_capture_file(
    conversation_id: &str,
    rel: &str,
    bytes: &[u8],
) -> Result<()> {
    let rel = rel
        .trim()
        .trim_start_matches(['/', '\\'])
        .replace('\\', "/");
    if rel.is_empty()
        || rel
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        bail!("invalid capture file name");
    }
    let store = capture_store(conversation_id)?;
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let conv_seg = sanitize_path_segment(conversation_id);
    let path = join_capture(&store.root, &format!("{date}/{conv_seg}/{rel}"));
    let parent = capture_parent(&path).context("capture file has no parent directory")?;
    store
        .env
        .create_dir_all(&parent)
        .with_context(|| format!("create capture dir {}", parent.display()))?;
    store
        .env
        .write_file(&path, bytes)
        .with_context(|| format!("write capture file {}", path.display()))?;
    Ok(())
}

pub(crate) fn purge_session_captures(conversation_id: &str) {
    let store = match capture_store(conversation_id) {
        Ok(store) => store,
        Err(error) => {
            log::warn!("computer capture purge: {error:#}");
            return;
        }
    };
    if let Err(error) = purge_in_store(&store, CAPTURE_RETENTION_DAYS) {
        log::warn!("computer capture purge: {error:#}");
    }
}

fn capture_dump_dir(conversation_id: &str) -> Option<(CaptureStore, String, PathBuf, i64)> {
    if !computer_capture_dump_enabled() {
        return None;
    }
    let store = match capture_store(conversation_id) {
        Ok(store) => store,
        Err(error) => {
            log::warn!("computer capture dump: {error:#}");
            return None;
        }
    };
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let ts = chrono::Local::now().timestamp_millis();
    let conv_seg = sanitize_path_segment(conversation_id);
    let dir = join_capture(&store.root, &format!("{date}/{conv_seg}"));
    if let Err(error) = store.env.create_dir_all(&dir) {
        log::warn!(
            "computer capture dump: create_dir_all {}: {error:#}",
            dir.display()
        );
        return None;
    }
    if let Err(error) = purge_in_store(&store, CAPTURE_RETENTION_DAYS) {
        log::warn!("computer capture purge: {error:#}");
    }
    Some((store, date, dir, ts))
}

/// Save one pipeline-phase image; on-disk stem matches the vision slot label.
/// `file_prefix` is `{conversationId}_{roundSeq}` (no phase; `phase_label` passed separately).
///
/// Returns path relative to `computer-captures/` (for UI lazy load).
pub fn save_pipeline_slot_image(
    conversation_id: &str,
    file_prefix: &str,
    phase_label: &str,
    slot_label: &str,
    ext: &str,
    bytes: &[u8],
) -> Option<String> {
    if bytes.is_empty() {
        return None;
    }
    let (store, date, dir, ts) = capture_dump_dir(conversation_id)?;
    let pfx = sanitize_path_segment(file_prefix);
    let stem = slot_label_to_disk_stem(slot_label);
    let fname = format!("{pfx}_{ts}_{phase_label}_{stem}.{ext}");
    let path = join_capture(&dir, &fname);
    store.env.write_file(&path, bytes).ok()?;
    let conv_seg = sanitize_path_segment(conversation_id);
    let rel = stored_capture_key(&date, &conv_seg, &fname)?;
    log::info!(
        "computer pipeline capture dump: slot={slot_label} rel={rel} ({} bytes)",
        bytes.len()
    );
    Some(rel)
}

/// Save multiple slot-labeled images for one pipeline phase (same timestamp).
pub fn save_pipeline_phase_captures(
    conversation_id: &str,
    file_prefix: &str,
    phase_label: &str,
    slots: &[(&str, &str, &[u8])],
) -> Option<String> {
    if slots.is_empty() {
        return None;
    }
    let (store, date, dir, ts) = capture_dump_dir(conversation_id)?;
    let pfx = sanitize_path_segment(file_prefix);
    let conv_seg = sanitize_path_segment(conversation_id);
    let mut last_rel = None;
    for (slot_label, ext, bytes) in slots {
        if bytes.is_empty() {
            log::warn!("computer pipeline capture dump: skip empty slot={slot_label}");
            continue;
        }
        let stem = slot_label_to_disk_stem(slot_label);
        let fname = format!("{pfx}_{ts}_{phase_label}_{stem}.{ext}");
        let path = join_capture(&dir, &fname);
        store.env.write_file(&path, bytes).ok()?;
        last_rel = stored_capture_key(&date, &conv_seg, &fname);
        log::info!(
            "computer pipeline capture dump: slot={slot_label} rel={} ({} bytes)",
            last_rel.as_deref().unwrap_or(""),
            bytes.len()
        );
    }
    last_rel
}

/// `{conv8}_{round:03}_{msg:03}` — the prefix every file of one collected
/// episode shares.
///
/// Collection is `glob("{prefix}_*")`: no filename parsing, no regex, no
/// timestamps. A sample therefore has to be identifiable from its name alone —
/// which round it came from and which step inside that round.
pub fn sample_prefix(conversation_id: &str, round_seq: u32, msg_seq: u32) -> String {
    let conv8: String = conversation_id.chars().take(8).collect();
    format!("{conv8}_{round_seq:03}_{msg_seq:03}")
}

/// Write one collected sample file: `{conv8}_{round}_{msg}_{stage}.{ext}`.
///
/// Separate from `write_bytes` because the collected naming has no timestamp —
/// `{prefix}_{stage}` is stable, so re-running a task overwrites the same file
/// instead of piling up `_1729...` duplicates. That is the point: the directory
/// is a dataset, not a log stream.
///
/// Returns `data:captures/{date}/{conversation}/{file}`.
pub fn write_sample_file(
    conversation_id: &str,
    round_seq: u32,
    msg_seq: u32,
    stage: &str,
    ext: &str,
    bytes: &[u8],
) -> Option<String> {
    if bytes.is_empty() {
        return None;
    }
    let (store, date, dir, _ts) = capture_dump_dir(conversation_id)?;
    let pfx = sample_prefix(conversation_id, round_seq, msg_seq);
    let fname = format!("{pfx}_{stage}.{ext}");
    let path = join_capture(&dir, &fname);
    if let Err(error) = store.env.write_file(&path, bytes) {
        log::warn!(
            "computer capture dump: sample {}: {error:#}",
            path.display()
        );
        return None;
    }
    let conv_seg = sanitize_path_segment(conversation_id);
    stored_capture_key(&date, &conv_seg, &fname)
}

/// Write one collected sample sidecar: `{conv8}_{round}_{msg}_{stage}.json`.
pub fn write_sample_json(
    conversation_id: &str,
    round_seq: u32,
    msg_seq: u32,
    stage: &str,
    value: &serde_json::Value,
) {
    let Ok(json) = serde_json::to_string_pretty(value) else {
        log::warn!("computer capture dump: sample json serialise failed");
        return;
    };
    let _ = write_sample_file(
        conversation_id,
        round_seq,
        msg_seq,
        stage,
        "json",
        json.as_bytes(),
    );
}

/// Writes one annotated frame for a conversation.
///
/// Local sessions use `{app_data}/computer-captures/{date}/{conversation}/`.
/// Sandbox sessions use `/data/captures/{date}/{conversation}/` in that environment.
///
/// Returns `data:captures/{date}/{conversation}/{file}` for lazy UI load.
pub fn save_computer_capture_debug(
    conversation_id: &str,
    file_prefix: &str,
    cap: &ScreenCaptureResult,
) -> Option<String> {
    if !computer_capture_dump_enabled() {
        return None;
    }
    if cap.screenshot_jpeg.is_empty() {
        log::warn!("computer capture dump: skip — empty annotated frame");
        return None;
    }

    let Some((store, date, dir, ts)) = capture_dump_dir(conversation_id) else {
        return None;
    };
    let conv_seg = sanitize_path_segment(conversation_id);
    let pfx = sanitize_path_segment(file_prefix);

    write_bytes(
        &store,
        &dir,
        &pfx,
        "screen",
        "jpg",
        ts,
        &cap.screenshot_jpeg,
    );

    let annotated_name = format!("{pfx}_{ts}_screen.jpg");
    let rel = stored_capture_key(&date, &conv_seg, &annotated_name)?;
    log::info!(
        "computer capture: id={rel} bytes={} dir={}",
        cap.screenshot_jpeg.len(),
        dir.display()
    );
    Some(rel)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_label_to_disk_stem_matches_pipeline_slots() {
        assert_eq!(
            slot_label_to_disk_stem("[Current screen]"),
            "current_screen"
        );
        assert_eq!(
            slot_label_to_disk_stem("[Annotated current screen]"),
            "annotated_current_screen"
        );
        assert_eq!(
            slot_label_to_disk_stem("[Screen before action]"),
            "screen_before_action"
        );
        assert_eq!(
            slot_label_to_disk_stem("[Screen after action]"),
            "screen_after_action"
        );
    }

    /// Sample names are what collection depends on: `glob("{prefix}_*")` has to
    /// pick up every file of one episode, so the prefix must be exactly
    /// `{conv8}_{round:03}_{msg:03}` — short conv id, zero-padded numbers.
    ///
    /// Deliberately a pure-function test. An earlier version wrote real files
    /// through the capture root, which meant swapping the process-global
    /// `data_dir()`; because Rust runs tests in parallel, that leaked into
    /// unrelated tests (`webhook_attachment` panicked on `strip_prefix` because
    /// it resolved storage into a temp dir that had already been deleted).
    #[test]
    fn sample_prefix_is_conv8_round_msg() {
        assert_eq!(sample_prefix("conv-abc123", 5, 3), "conv-abc_005_003");
        // Short ids are not padded: there is nothing to pad.
        assert_eq!(sample_prefix("abc", 1, 1), "abc_001_001");
        // Long ids are cut to 8 chars so the name stays readable.
        assert_eq!(sample_prefix("0123456789abcdef", 12, 7), "01234567_012_007");
        // Round / msg are zero-padded to 3 so lexical sort == numeric sort.
        assert_eq!(sample_prefix("c", 9, 99), "c_009_099");
    }

    #[test]
    fn guest_capture_path_stays_under_data() {
        let path = join_capture(Path::new("/data/captures"), "2026-01-02/conv/a.jpg");
        assert_eq!(path_text(&path), "/data/captures/2026-01-02/conv/a.jpg");
        assert_eq!(
            capture_parent(&path).as_deref().map(path_text),
            Some("/data/captures/2026-01-02/conv".to_string())
        );
    }

    #[test]
    fn capture_rel_rejects_parent_segments() {
        assert!(
            checked_capture_rel(Path::new("/data/captures"), "2026-01-02/conv/../../x").is_none()
        );
        assert!(capture_conversation_id("locate/a.jpg").is_none());
        assert_eq!(
            capture_conversation_id("2026-01-02/conv-1/screen.jpg").as_deref(),
            Some("conv-1")
        );
        assert_eq!(
            capture_conversation_id("data:captures/2026-01-02/conv-1/screen.jpg").as_deref(),
            Some("conv-1")
        );
        assert!(capture_conversation_id("/Users/someone/secret.pdf").is_none());
    }

    #[test]
    fn absolute_capture_key_is_not_opened() {
        let secret =
            std::env::temp_dir().join(format!("pointer-capture-secret-{}", std::process::id()));
        std::fs::write(&secret, b"host-secret").unwrap();
        let err = read_computer_capture_preview(&secret.to_string_lossy()).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("宿主绝对路径") || msg.contains("invalid"),
            "{msg}"
        );
        assert!(!msg.contains("host-secret"), "{msg}");
        let _ = std::fs::remove_file(&secret);
    }
}
