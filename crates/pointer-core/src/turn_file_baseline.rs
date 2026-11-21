//! Per-turn file content baselines for review (first read before edit/write).
//!
//! Storage: `{app_data}/turn-baselines/{conversation_id}/{turn_id}/{sha256(path)}.txt`
//! plus a sibling `.path` file recording the absolute path for debugging,
//! and `summary.json` for the page-footer file list (written once when the UI
//! freezes the turn — same rows the user already sees).
//!
//! On the first successful mutating file tool in a turn for a path, callers persist
//! the pre-write content. Review diffs that snapshot against the next later
//! baseline for the same path, or the on-disk file when none exists.

use crate::storage;
use crate::text_diff::compute_diff_lines;
use anyhow::{anyhow, bail, Context, Result};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

/// Skip baselines larger than this (bytes) to avoid blowing app-data disk.
pub const MAX_BASELINE_BYTES: usize = 5 * 1024 * 1024;

thread_local! {
    static TURN_BASELINE_CTX: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
}

/// Lead user message id for the active turn, keyed by conversation.
/// File tools reuse this for the whole turn. The next lead `run_chat` replaces it.
static ACTIVE_TURN_IDS: LazyLock<Mutex<HashMap<String, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn turn_id_map() -> std::sync::MutexGuard<'static, HashMap<String, String>> {
    ACTIVE_TURN_IDS.lock().unwrap_or_else(|poisoned| {
        warn!("turn_file_baseline: turn-id map lock poisoned; continuing with inner data");
        poisoned.into_inner()
    })
}

/// Remember the lead user message id for this conversation's active turn.
pub fn remember_active_turn_id(conversation_id: &str, turn_id: &str) {
    let conversation_id = conversation_id.trim();
    let turn_id = turn_id.trim();
    if conversation_id.is_empty() || turn_id.is_empty() {
        warn!(
            "turn_file_baseline: skip pin empty id conversation_id={conversation_id} turn_id={turn_id}"
        );
        return;
    }
    let mut map = turn_id_map();
    if map.get(conversation_id).map(String::as_str) == Some(turn_id) {
        return;
    }
    info!("turn_file_baseline: pin turn conversation_id={conversation_id} turn_id={turn_id}");
    map.insert(conversation_id.to_string(), turn_id.to_string());
}

fn pinned_turn_id(conversation_id: &str) -> Option<String> {
    let conversation_id = conversation_id.trim();
    if conversation_id.is_empty() {
        return None;
    }
    turn_id_map().get(conversation_id).cloned()
}

/// Sets conversation + turn ids for baseline capture on the current worker thread.
pub struct TurnBaselineGuard {
    previous: Option<(String, String)>,
}

impl TurnBaselineGuard {
    pub fn enter(conversation_id: impl Into<String>, turn_id: impl Into<String>) -> Self {
        let conversation_id = conversation_id.into();
        let turn_id = turn_id.into();
        let next = if conversation_id.trim().is_empty() || turn_id.trim().is_empty() {
            None
        } else {
            Some((conversation_id, turn_id))
        };
        let previous =
            TURN_BASELINE_CTX.with(|cell| std::mem::replace(&mut *cell.borrow_mut(), next));
        Self { previous }
    }
}

impl Drop for TurnBaselineGuard {
    fn drop(&mut self) {
        TURN_BASELINE_CTX.with(|cell| {
            *cell.borrow_mut() = self.previous.take();
        });
    }
}

fn current_ctx() -> Option<(String, String)> {
    TURN_BASELINE_CTX.with(|cell| cell.borrow().clone())
}

fn baseline_root(conversation_id: &str, turn_id: &str) -> Result<PathBuf> {
    let conv = sanitize_id(conversation_id);
    let turn = sanitize_id(turn_id);
    if conv.is_empty() || turn.is_empty() {
        return Err(anyhow!("conversation_id and turn_id are required"));
    }
    Ok(storage::app_data_dir()?
        .join("turn-baselines")
        .join(conv)
        .join(turn))
}

fn sanitize_id(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn path_key(abs_path: &Path) -> String {
    let normalized = abs_path.to_string_lossy().replace('\\', "/").to_lowercase();
    let mut hasher = Sha256::new();
    hasher.update(normalized.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn content_path(root: &Path, abs_path: &Path) -> PathBuf {
    root.join(format!("{}.txt", path_key(abs_path)))
}

fn meta_path(root: &Path, abs_path: &Path) -> PathBuf {
    root.join(format!("{}.path", path_key(abs_path)))
}

fn summary_json_path(root: &Path) -> PathBuf {
    root.join("summary.json")
}

/// One path changed in a lead turn (page-footer summary / reload hydrate).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TurnFileChangeEntry {
    pub path: String,
    /// `edit` or `write`.
    pub kind: String,
    pub adds: u64,
    pub dels: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct TurnFileChangeSummaryFile {
    files: Vec<TurnFileChangeEntry>,
}

/// Batch list result: one row per requested turn id (missing turns → empty files).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TurnFileChangesForTurn {
    pub turn_id: String,
    pub files: Vec<TurnFileChangeEntry>,
}

fn normalize_kind(kind: &str) -> String {
    if kind.trim().eq_ignore_ascii_case("write") {
        "write".into()
    } else {
        "edit".into()
    }
}

fn normalize_entries(files: &[TurnFileChangeEntry]) -> Vec<TurnFileChangeEntry> {
    let mut by_path: BTreeMap<String, TurnFileChangeEntry> = BTreeMap::new();
    for file in files {
        let path = file.path.trim().replace('\\', "/");
        if path.is_empty() {
            continue;
        }
        let key = path.to_lowercase();
        let incoming = TurnFileChangeEntry {
            path,
            kind: normalize_kind(&file.kind),
            adds: file.adds,
            dels: file.dels,
        };
        if let Some(prev) = by_path.remove(&key) {
            let kind = if prev.kind == "write" || incoming.kind == "write" {
                "write".into()
            } else {
                "edit".into()
            };
            by_path.insert(
                key,
                TurnFileChangeEntry {
                    path: incoming.path,
                    kind,
                    adds: prev.adds.saturating_add(incoming.adds),
                    dels: prev.dels.saturating_add(incoming.dels),
                },
            );
        } else {
            by_path.insert(key, incoming);
        }
    }
    let mut out: Vec<_> = by_path.into_values().collect();
    out.sort_by(|a, b| {
        a.path
            .to_lowercase()
            .cmp(&b.path.to_lowercase())
            .then_with(|| a.path.cmp(&b.path))
    });
    out
}

/// Persist the UI-frozen footer list for one lead turn (one `summary.json`).
/// Call when the turn freezes — same rows already shown in the page footer.
pub fn save_turn_file_changes(
    conversation_id: &str,
    turn_id: &str,
    files: &[TurnFileChangeEntry],
) -> Result<()> {
    let conv = conversation_id.trim();
    let turn = turn_id.trim();
    if conv.is_empty() || turn.is_empty() {
        return Err(anyhow!("conversation_id and turn_id are required"));
    }
    let normalized = normalize_entries(files);
    if normalized.is_empty() {
        info!("turn_file_baseline: skip empty summary conversation_id={conv} turn_id={turn}");
        return Ok(());
    }
    let root = baseline_root(conv, turn)?;
    fs::create_dir_all(&root)
        .with_context(|| format!("create turn baseline dir {}", root.display()))?;
    let file = summary_json_path(&root);
    let payload = TurnFileChangeSummaryFile {
        files: normalized.clone(),
    };
    let json = serde_json::to_string_pretty(&payload)
        .with_context(|| format!("serialize turn summary {}", file.display()))?;
    fs::write(&file, json.as_bytes())
        .with_context(|| format!("write turn summary {}", file.display()))?;
    info!(
        "turn_file_baseline: saved summary conversation_id={conv} turn_id={turn} files={}",
        normalized.len()
    );
    Ok(())
}

fn list_one_turn_file_changes(
    conversation_id: &str,
    turn_id: &str,
) -> Result<Vec<TurnFileChangeEntry>> {
    let root = baseline_root(conversation_id, turn_id)?;
    if !root.exists() {
        return Ok(Vec::new());
    }
    let summary = summary_json_path(&root);
    if summary.exists() {
        let text = fs::read_to_string(&summary)
            .with_context(|| format!("read turn summary {}", summary.display()))?;
        match serde_json::from_str::<TurnFileChangeSummaryFile>(&text) {
            Ok(parsed) => return Ok(normalize_entries(&parsed.files)),
            Err(error) => {
                warn!(
                    "turn_file_baseline: invalid summary conversation_id={conversation_id} turn_id={turn_id}: {error}"
                );
            }
        }
    }
    // Legacy / no UI freeze yet: paths from baseline sidecars (no +/-).
    let mut by_key: BTreeMap<String, TurnFileChangeEntry> = BTreeMap::new();
    let entries = fs::read_dir(&root)
        .with_context(|| format!("read turn baseline dir {}", root.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("read dir entry under {}", root.display()))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if let Some(key) = name.strip_suffix(".path") {
            let path_text = fs::read_to_string(entry.path()).unwrap_or_default();
            let path = path_text.trim().replace('\\', "/");
            if path.is_empty() {
                continue;
            }
            by_key.insert(
                key.to_string(),
                TurnFileChangeEntry {
                    path,
                    kind: "edit".into(),
                    adds: 0,
                    dels: 0,
                },
            );
        }
    }
    let mut files: Vec<_> = by_key.into_values().collect();
    files.sort_by(|a, b| {
        a.path
            .to_lowercase()
            .cmp(&b.path.to_lowercase())
            .then_with(|| a.path.cmp(&b.path))
    });
    Ok(files)
}

/// List changed files for one or more lead turns (reload hydrate / empty message scan).
/// Prefers UI-frozen `summary.json`; otherwise falls back to `.path` sidecars.
/// Does not load conversation messages.
pub fn list_turn_file_changes(
    conversation_id: &str,
    turn_ids: &[String],
) -> Result<Vec<TurnFileChangesForTurn>> {
    let conv = conversation_id.trim();
    if conv.is_empty() {
        return Err(anyhow!("conversation_id is required"));
    }
    let mut out = Vec::with_capacity(turn_ids.len());
    for turn_id in turn_ids {
        let turn = turn_id.trim();
        if turn.is_empty() {
            warn!("turn_file_baseline: skip empty turn_id in list_turn_file_changes");
            continue;
        }
        let files = list_one_turn_file_changes(conv, turn)?;
        info!(
            "turn_file_baseline: list changes conversation_id={conv} turn_id={turn} files={}",
            files.len()
        );
        out.push(TurnFileChangesForTurn {
            turn_id: turn.to_string(),
            files,
        });
    }
    Ok(out)
}

/// Persist pre-write content once per (conversation, turn, path). No-op if ctx unset or already stored.
pub fn ensure_baseline(abs_path: &Path, content_before: &str) -> Result<()> {
    let Some((conversation_id, turn_id)) = current_ctx() else {
        warn!(
            "turn_file_baseline: skip ensure (no turn context) path={}",
            abs_path.display()
        );
        return Ok(());
    };
    if content_before.len() > MAX_BASELINE_BYTES {
        warn!(
            "turn_file_baseline: skip oversized baseline conversation_id={conversation_id} turn_id={turn_id} path={} bytes={}",
            abs_path.display(),
            content_before.len()
        );
        return Ok(());
    }
    let root = baseline_root(&conversation_id, &turn_id)?;
    fs::create_dir_all(&root)
        .with_context(|| format!("create turn baseline dir {}", root.display()))?;
    let file = content_path(&root, abs_path);
    if file.exists() {
        return Ok(());
    }
    fs::write(&file, content_before.as_bytes())
        .with_context(|| format!("write turn baseline {}", file.display()))?;
    let _ = fs::write(
        meta_path(&root, abs_path),
        abs_path.to_string_lossy().as_bytes(),
    );
    info!(
        "turn_file_baseline: saved conversation_id={conversation_id} turn_id={turn_id} path={} bytes={}",
        abs_path.display(),
        content_before.len()
    );
    Ok(())
}

fn read_baseline_content(
    conversation_id: &str,
    turn_id: &str,
    abs_path: &Path,
) -> Result<Option<String>> {
    let root = baseline_root(conversation_id, turn_id)?;
    let file = content_path(&root, abs_path);
    if !file.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(&file)
        .with_context(|| format!("read turn baseline {}", file.display()))?;
    Ok(Some(text))
}

fn read_baseline_either(
    conversation_id: &str,
    turn_id: &str,
    abs: &Path,
    raw_abs: &Path,
) -> Result<Option<String>> {
    match read_baseline_content(conversation_id, turn_id, abs)? {
        Some(text) => Ok(Some(text)),
        None if raw_abs.as_os_str() != abs.as_os_str() => {
            read_baseline_content(conversation_id, turn_id, raw_abs)
        }
        None => Ok(None),
    }
}

fn load_subsequent_lead_turn_ids(conversation_id: &str, turn_id: &str) -> Vec<String> {
    match crate::conversation_store::global_store() {
        Ok(store) => match store.load_subsequent_lead_turn_ids(conversation_id, turn_id) {
            Ok(ids) => ids,
            Err(error) => {
                warn!(
                    "turn_file_baseline: load_subsequent_lead_turn_ids failed conversation_id={conversation_id} turn_id={turn_id}: {error:#}"
                );
                Vec::new()
            }
        },
        Err(error) => {
            warn!(
                "turn_file_baseline: global_store unavailable for next baseline conversation_id={conversation_id}: {error:#}"
            );
            Vec::new()
        }
    }
}

fn read_next_path_baseline(
    conversation_id: &str,
    subsequent_turn_ids: &[String],
    abs: &Path,
    raw_abs: &Path,
) -> Result<Option<(String, String)>> {
    for next_turn in subsequent_turn_ids {
        if let Some(text) = read_baseline_either(conversation_id, next_turn, abs, raw_abs)? {
            return Ok(Some((next_turn.clone(), text)));
        }
    }
    Ok(None)
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn path_needs_session_env(workspace_root: &Path, path: &str) -> bool {
    crate::workspace_resolve::looks_like_sandbox_guest_path(&path_text(workspace_root))
        || crate::workspace_resolve::looks_like_sandbox_guest_path(path)
}

/// 沙箱路径走已缓存的会话环境。本机路径走宿主。
///
/// 不调用 `effective_env()`：沙箱部署下未绑定会得到 `DeniedEnv`。
/// 也不在会话环境缺失时改读宿主上的 data 卷。
fn env_for_turn_file_diff(
    conversation_id: &str,
    workspace_root: &Path,
    path: &str,
) -> Result<Arc<dyn crate::env::Environment>> {
    let guest = path_needs_session_env(workspace_root, path);
    if let Some(env) = crate::env::session_env::env_for_conversation(conversation_id) {
        let sandbox = !env.is_denied() && !env.path_map().is_host_passthrough();
        if guest {
            if !sandbox {
                warn!(
                    "turn_file_diff: session environment unusable conversation_id={conversation_id} workspace={} path={path}",
                    workspace_root.display()
                );
                bail!(
                    "沙箱会话的执行环境不可用，无法读取回合文件（conversation={conversation_id}）"
                );
            }
            info!(
                "turn_file_diff: session environment conversation_id={conversation_id} workspace={} path={path}",
                workspace_root.display()
            );
            return Ok(env);
        }
        if env.kind() == crate::env::EnvKind::Local
            || (env.path_map().is_host_passthrough() && !env.is_denied())
        {
            info!(
                "turn_file_diff: session environment conversation_id={conversation_id} workspace={} path={path}",
                workspace_root.display()
            );
            return Ok(env);
        }
        info!(
            "turn_file_diff: host path via host environment conversation_id={conversation_id} workspace={} path={path}",
            workspace_root.display()
        );
        return Ok(crate::env::host_env_at(workspace_root));
    }
    if guest {
        warn!(
            "turn_file_diff: sandbox path has no session environment conversation_id={conversation_id} workspace={} path={path}",
            workspace_root.display()
        );
        bail!(
            "工作区在沙箱内（{}），但当前没有会话环境",
            workspace_root.display()
        );
    }
    Ok(crate::env::host_env_at(workspace_root))
}

fn join_guest(root: &Path, relative: &str) -> PathBuf {
    let root = path_text(root);
    let root = root.trim_end_matches('/');
    let relative = relative.trim().replace('\\', "/");
    let relative = relative.trim_start_matches('/');
    PathBuf::from(format!("{root}/{relative}"))
}

fn resolve_diff_path(
    workspace_root: &Path,
    path: &str,
    canonicalize_on_host: bool,
) -> Result<PathBuf> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("path is required"));
    }
    let guest_file = crate::workspace_resolve::looks_like_sandbox_guest_path(trimmed);
    let absolute = guest_file || Path::new(trimmed).is_absolute();
    let joined = if !absolute {
        if crate::workspace_resolve::looks_like_sandbox_guest_path(&path_text(workspace_root)) {
            join_guest(workspace_root, trimmed)
        } else {
            workspace_root.join(trimmed)
        }
    } else if guest_file {
        PathBuf::from(trimmed.replace('\\', "/"))
    } else {
        PathBuf::from(trimmed)
    };
    if canonicalize_on_host && !guest_file {
        return Ok(joined.canonicalize().unwrap_or(joined));
    }
    Ok(joined)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TurnFileDiff {
    pub path: String,
    pub baseline_missing: bool,
    pub created: bool,
    pub diff_lines: Vec<serde_json::Value>,
    pub diff_stats: serde_json::Value,
}

/// Diff this turn's baseline against the next later baseline for the same path,
/// or the current file when no later snapshot exists.
///
/// Web and the desktop client both call this. Guest `/data/...` paths are read
/// through the cached session environment. Host paths are read on the host.
/// Relative paths join `workspace_root` from the caller.
pub fn turn_file_diff(
    conversation_id: &str,
    turn_id: &str,
    workspace_root: &Path,
    path: &str,
) -> Result<TurnFileDiff> {
    let env = env_for_turn_file_diff(conversation_id, workspace_root, path)?;
    let on_host = env.kind() == crate::env::EnvKind::Local || env.path_map().is_host_passthrough();
    let root = if on_host {
        workspace_root.canonicalize().with_context(|| {
            format!("workspaceRoot does not exist: {}", workspace_root.display())
        })?
    } else {
        workspace_root.to_path_buf()
    };
    let abs = resolve_diff_path(&root, path, on_host)?;
    let display_path = abs.to_string_lossy().replace('\\', "/");
    // Tool results often store a non-canonical absolute path; try both keys.
    let raw_abs = PathBuf::from(path.trim());
    let baseline = read_baseline_either(conversation_id, turn_id, &abs, &raw_abs)?;
    let baseline_missing = baseline.is_none();
    let before = baseline.unwrap_or_default();

    let subsequent = load_subsequent_lead_turn_ids(conversation_id, turn_id);
    let next_baseline = read_next_path_baseline(conversation_id, &subsequent, &abs, &raw_abs)?;
    let after = if let Some((next_turn_id, text)) = next_baseline {
        info!(
            "turn_file_baseline: after=next_baseline conversation_id={conversation_id} turn_id={turn_id} next_turn_id={next_turn_id} path={display_path}"
        );
        text
    } else if env.exists(&abs) {
        let meta = env
            .metadata(&abs)
            .with_context(|| format!("stat {}", abs.display()))?;
        if !meta.is_file {
            return Err(anyhow!("不是常规文件: {}", abs.display()));
        }
        env.read_to_string(&abs)
            .with_context(|| format!("read {}", abs.display()))?
    } else {
        String::new()
    };

    // File did not exist at baseline (empty snapshot) and has content after this turn.
    let created = before.is_empty() && !after.is_empty() && !baseline_missing;

    let (diff_lines, diff_stats) = compute_diff_lines(&before, &after);
    if baseline_missing {
        warn!(
            "turn_file_baseline: missing baseline conversation_id={conversation_id} turn_id={turn_id} path={display_path}"
        );
    } else {
        info!(
            "turn_file_baseline: diff conversation_id={conversation_id} turn_id={turn_id} path={display_path}"
        );
    }

    Ok(TurnFileDiff {
        path: display_path,
        baseline_missing,
        created,
        diff_lines,
        diff_stats,
    })
}

/// Resolve the active user-turn id for baseline capture.
///
/// Reuses the id pinned at lead `run_chat` start. On a miss (process restart,
/// or a write before the pin), reads the latest real lead user id from the tail
/// and pins that. Does not load the full transcript.
pub fn resolve_active_turn_id(conversation_id: &str) -> String {
    if let Some(turn_id) = pinned_turn_id(conversation_id) {
        return turn_id;
    }
    match crate::conversation_store::global_store() {
        Ok(store) => match store.load_latest_real_lead_user_message_id(conversation_id) {
            Ok(Some(turn_id)) => {
                remember_active_turn_id(conversation_id, &turn_id);
                turn_id
            }
            Ok(None) => {
                warn!(
                    "turn_file_baseline: no user turn in history conversation_id={conversation_id}"
                );
                format!("orphan-{conversation_id}")
            }
            Err(error) => {
                warn!(
                    "turn_file_baseline: latest lead user lookup failed conversation_id={conversation_id}: {error:#}"
                );
                format!("orphan-{conversation_id}")
            }
        },
        Err(error) => {
            warn!(
                "turn_file_baseline: global_store unavailable conversation_id={conversation_id}: {error:#}"
            );
            format!("orphan-{conversation_id}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;

    #[test]
    fn compute_diff_detects_insert() {
        let (lines, stats) = compute_diff_lines("a\n", "a\nb\n");
        assert_eq!(stats["adds"].as_u64().unwrap(), 1);
        assert!(lines.iter().any(|line| line["type"] == "ins"));
    }

    #[test]
    fn path_key_stable_for_slash_variants() {
        let a = path_key(Path::new("/tmp/Foo/bar.ts"));
        let b = path_key(Path::new("/tmp/Foo\\bar.ts"));
        assert_eq!(a, b);
    }

    #[test]
    fn next_path_baseline_stops_at_first_existing() {
        let _lock = crate::storage::test_app_data_dir_lock();
        let dir = tempfile::tempdir().unwrap();
        crate::storage::set_test_app_data_dir(dir.path().to_path_buf());
        let path = Path::new("/ws/file.ts");
        {
            let _guard = TurnBaselineGuard::enter("conv", "u2");
            ensure_baseline(path, "after-u1").unwrap();
        }
        {
            let _guard = TurnBaselineGuard::enter("conv", "u3");
            ensure_baseline(path, "after-u2").unwrap();
        }
        let found =
            read_next_path_baseline("conv", &["u2".into(), "u3".into()], path, path).unwrap();
        assert_eq!(found, Some(("u2".into(), "after-u1".into())));
    }

    #[test]
    fn change_summary_save_and_list() {
        let _lock = crate::storage::test_app_data_dir_lock();
        let dir = tempfile::tempdir().unwrap();
        crate::storage::set_test_app_data_dir(dir.path().to_path_buf());
        save_turn_file_changes(
            "conv",
            "u1",
            &[
                TurnFileChangeEntry {
                    path: "/ws/b.ts".into(),
                    kind: "edit".into(),
                    adds: 2,
                    dels: 1,
                },
                TurnFileChangeEntry {
                    path: "/ws/a.ts".into(),
                    kind: "write".into(),
                    adds: 4,
                    dels: 0,
                },
            ],
        )
        .unwrap();
        let listed = list_turn_file_changes("conv", &["u1".into(), "missing".into()]).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].turn_id, "u1");
        assert_eq!(listed[0].files.len(), 2);
        assert_eq!(listed[0].files[0].path, "/ws/a.ts");
        assert_eq!(listed[0].files[0].kind, "write");
        assert_eq!(listed[0].files[0].adds, 4);
        assert_eq!(listed[0].files[1].path, "/ws/b.ts");
        assert_eq!(listed[0].files[1].adds, 2);
        assert!(listed[1].files.is_empty());
    }

    #[test]
    fn change_list_falls_back_to_path_sidecar() {
        let _lock = crate::storage::test_app_data_dir_lock();
        let dir = tempfile::tempdir().unwrap();
        crate::storage::set_test_app_data_dir(dir.path().to_path_buf());
        let path = Path::new("/ws/legacy.ts");
        {
            let _guard = TurnBaselineGuard::enter("conv", "u1");
            ensure_baseline(path, "x").unwrap();
        }
        let listed = list_turn_file_changes("conv", &["u1".into()]).unwrap();
        assert_eq!(listed[0].files.len(), 1);
        assert_eq!(listed[0].files[0].path, "/ws/legacy.ts");
        assert_eq!(listed[0].files[0].adds, 0);
    }

    #[test]
    fn resolve_reuses_pinned_turn_id() {
        let conv = format!("pin-{}", uuid::Uuid::new_v4());
        remember_active_turn_id(&conv, "user-1");
        assert_eq!(resolve_active_turn_id(&conv), "user-1");
        remember_active_turn_id(&conv, "user-2");
        assert_eq!(resolve_active_turn_id(&conv), "user-2");
        remember_active_turn_id("", "user-3");
        remember_active_turn_id(&conv, "  ");
        assert_eq!(resolve_active_turn_id(&conv), "user-2");
    }

    #[test]
    fn resolve_without_pin_does_not_cache_orphan() {
        let conv = format!("miss-{}", uuid::Uuid::new_v4());
        assert_eq!(resolve_active_turn_id(&conv), format!("orphan-{conv}"));
        assert!(!turn_id_map().contains_key(&conv));
    }

    #[test]
    fn host_diff_reads_the_caller_workspace() {
        let _lock = crate::storage::test_app_data_dir_lock();
        let dir = tempfile::tempdir().unwrap();
        crate::storage::set_test_app_data_dir(dir.path().join("app"));
        let workspace = dir.path().join("ws");
        fs::create_dir_all(workspace.join("src")).unwrap();
        let file = workspace.join("src/a.txt");
        fs::write(&file, "now\n").unwrap();
        let abs = file.canonicalize().unwrap();
        let conv = format!("host-diff-{}", uuid::Uuid::new_v4());
        {
            let _guard = TurnBaselineGuard::enter(&conv, "u1");
            ensure_baseline(&abs, "old\n").unwrap();
        }
        let diff = turn_file_diff(&conv, "u1", &workspace, "src/a.txt").unwrap();
        assert!(!diff.baseline_missing);
        assert_eq!(diff.diff_stats["adds"].as_u64().unwrap(), 1);
    }

    #[test]
    fn guest_diff_without_session_env_errors() {
        let conv = format!("guest-miss-{}", uuid::Uuid::new_v4());
        let err = turn_file_diff(&conv, "u1", Path::new("/data/workspaces/proj"), "src/a.txt")
            .unwrap_err();
        assert!(err.to_string().contains("没有会话环境"), "{err}");
    }

    #[test]
    fn guest_diff_rejects_a_host_passthrough_env() {
        let conv = format!("guest-local-{}", uuid::Uuid::new_v4());
        crate::env::session_env::remember_local_for_test(&conv, Path::new("/data/workspaces"));
        let err = turn_file_diff(&conv, "u1", Path::new("/data/workspaces/proj"), "src/a.txt")
            .unwrap_err();
        assert!(err.to_string().contains("执行环境不可用"), "{err}");
    }

    #[test]
    fn guest_diff_reads_the_caller_project_not_the_env_workdir() {
        let _lock = crate::storage::test_app_data_dir_lock();
        let dir = tempfile::tempdir().unwrap();
        crate::storage::set_test_app_data_dir(dir.path().join("app"));
        let conv = format!("guest-diff-{}", uuid::Uuid::new_v4());
        let project = "/data/workspaces/proj/src/a.rs";
        let env = GuestDiffEnv::new("/data/workspaces");
        env.put(project, b"now\n");
        crate::env::session_env::remember_env_for_test(&conv, std::sync::Arc::new(env));
        {
            let _guard = TurnBaselineGuard::enter(&conv, "u1");
            ensure_baseline(Path::new(project), "old\n").unwrap();
        }
        let diff =
            turn_file_diff(&conv, "u1", Path::new("/data/workspaces/proj"), "src/a.rs").unwrap();
        assert_eq!(diff.path, project);
        assert!(!diff.baseline_missing);
        assert_eq!(diff.diff_stats["adds"].as_u64().unwrap(), 1);
    }

    struct GuestDiffEnv {
        workspace: PathBuf,
        map: crate::env::PathMap,
        files: std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>,
    }

    impl GuestDiffEnv {
        fn new(workspace: &str) -> Self {
            Self {
                workspace: PathBuf::from(workspace),
                map: crate::env::PathMap::new(vec![crate::env::Mount::new(
                    "/tmp/pointer-data",
                    "/data",
                    false,
                )]),
                files: Mutex::new(HashMap::new()),
            }
        }

        fn put(&self, path: &str, bytes: &[u8]) {
            self.files
                .lock()
                .expect("files")
                .insert(path.replace('\\', "/"), bytes.to_vec());
        }

        fn lookup(&self, path: &Path) -> Option<Vec<u8>> {
            let key = path.to_string_lossy().replace('\\', "/");
            self.files.lock().expect("files").get(&key).cloned()
        }
    }

    impl crate::env::Environment for GuestDiffEnv {
        fn kind(&self) -> crate::env::EnvKind {
            crate::env::EnvKind::Headless
        }
        fn workspace_root(&self) -> &Path {
            &self.workspace
        }
        fn skills_root(&self) -> &Path {
            Path::new("/skills")
        }
        fn home_root(&self) -> &Path {
            Path::new("/home/pointer")
        }
        fn capabilities(&self) -> crate::env::Capabilities {
            crate::env::Capabilities::headless()
        }
        fn path_map(&self) -> &crate::env::PathMap {
            &self.map
        }
        fn exec(
            &self,
            _argv: &[String],
            _opts: crate::env::ExecOptions,
        ) -> Result<crate::env::ExecOutput> {
            Err(anyhow::anyhow!("guest diff env: exec"))
        }
        fn spawn(
            &self,
            _argv: &[String],
            _opts: crate::env::SpawnOptions,
        ) -> Result<Box<dyn crate::env::ChildHandle>> {
            Err(anyhow::anyhow!("guest diff env: spawn"))
        }
        fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
            self.lookup(path)
                .ok_or_else(|| anyhow::anyhow!("guest diff env: missing {}", path.display()))
        }
        fn write_file(&self, _path: &Path, _data: &[u8]) -> Result<()> {
            Ok(())
        }
        fn list_dir(&self, _path: &Path) -> Result<Vec<crate::env::DirEntry>> {
            Ok(Vec::new())
        }
        fn metadata(&self, path: &Path) -> Result<crate::env::FileMetadata> {
            let bytes = self
                .lookup(path)
                .ok_or_else(|| anyhow::anyhow!("guest diff env: missing {}", path.display()))?;
            Ok(crate::env::FileMetadata {
                is_file: true,
                is_dir: false,
                is_symlink: false,
                len: bytes.len() as u64,
            })
        }
        fn canonicalize(&self, path: &Path) -> Result<PathBuf> {
            Ok(path.to_path_buf())
        }
        fn create_dir_all(&self, _path: &Path) -> Result<()> {
            Ok(())
        }
        fn read_to_string(&self, path: &Path) -> Result<String> {
            Ok(String::from_utf8(self.read_file(path)?)?)
        }
        fn open_reader(&self, path: &Path) -> Result<Box<dyn std::io::Read + Send>> {
            Ok(Box::new(std::io::Cursor::new(self.read_file(path)?)))
        }
        fn walk(
            &self,
            _root: &Path,
            _opts: crate::env::WalkOptions,
        ) -> Result<Vec<crate::env::WalkEntry>> {
            Ok(Vec::new())
        }
        fn remove_file(&self, _path: &Path) -> Result<()> {
            Ok(())
        }
        fn remove_dir_all(&self, _path: &Path) -> Result<()> {
            Ok(())
        }
        fn set_executable(&self, _path: &Path) -> Result<()> {
            Ok(())
        }
    }
}
