//! Per-turn file content baselines for review (first read before edit/write).
//!
//! Storage: `{app_data}/turn-baselines/{conversation_id}/{turn_id}/{sha256(path)}.txt`
//! plus a sibling `.path` file recording the absolute path for debugging.
//!
//! On the first successful mutating file tool in a turn for a path, callers persist
//! the pre-write content. UI later diffs that snapshot against the on-disk file.

use crate::storage;
use crate::text_diff::compute_diff_lines;
use anyhow::{anyhow, Context, Result};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};

/// Skip baselines larger than this (bytes) to avoid blowing app-data disk.
pub const MAX_BASELINE_BYTES: usize = 5 * 1024 * 1024;

thread_local! {
    static TURN_BASELINE_CTX: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
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
        let previous = TURN_BASELINE_CTX.with(|cell| std::mem::replace(&mut *cell.borrow_mut(), next));
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

fn resolve_abs_path(workspace_root: &Path, path: &str) -> Result<PathBuf> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("path is required"));
    }
    let candidate = PathBuf::from(trimmed);
    if candidate.is_absolute() {
        return Ok(candidate
            .canonicalize()
            .unwrap_or_else(|_| candidate));
    }
    let joined = workspace_root.join(trimmed);
    Ok(joined.canonicalize().unwrap_or(joined))
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

/// Diff turn baseline (first pre-write content) against the current on-disk file.
pub fn turn_file_diff(
    conversation_id: &str,
    turn_id: &str,
    workspace_root: &Path,
    path: &str,
) -> Result<TurnFileDiff> {
    let root = workspace_root
        .canonicalize()
        .with_context(|| format!("workspaceRoot does not exist: {}", workspace_root.display()))?;
    let abs = resolve_abs_path(&root, path)?;
    let display_path = abs.to_string_lossy().replace('\\', "/");
    // Tool results often store a non-canonical absolute path; try both keys.
    let raw_abs = PathBuf::from(path.trim());
    let baseline = match read_baseline_content(conversation_id, turn_id, &abs)? {
        Some(text) => Some(text),
        None if raw_abs.as_os_str() != abs.as_os_str() => {
            read_baseline_content(conversation_id, turn_id, &raw_abs)?
        }
        None => None,
    };
    let baseline_missing = baseline.is_none();
    let before = baseline.unwrap_or_default();

    let after = if abs.exists() {
        let meta = fs::metadata(&abs).with_context(|| format!("stat {}", abs.display()))?;
        if !meta.is_file() {
            return Err(anyhow!("不是常规文件: {}", abs.display()));
        }
        fs::read_to_string(&abs).with_context(|| format!("read {}", abs.display()))?
    } else {
        String::new()
    };

    // File did not exist at baseline (empty snapshot) and exists now.
    let created = before.is_empty() && abs.exists() && !baseline_missing;

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

/// Resolve the active user-turn id for baseline capture (latest real user message).
pub fn resolve_active_turn_id(conversation_id: &str) -> String {
    match crate::conversation_store::global_store() {
        Ok(store) => match store.load_messages(conversation_id) {
            Ok(messages) => crate::task_board::latest_real_user_message_id(&messages)
                .unwrap_or_else(|| {
                    warn!(
                        "turn_file_baseline: no user turn in history conversation_id={conversation_id}"
                    );
                    format!("orphan-{conversation_id}")
                }),
            Err(error) => {
                warn!(
                    "turn_file_baseline: load_messages failed conversation_id={conversation_id}: {error:#}"
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
}
