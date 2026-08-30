//! Parallel tool-call metadata, conflict keys, and default concurrency limits.

use crate::agents::computer::input::timing::desktop_tool_family_id;
use crate::tools::file::{
    resolve_accessible_path, resolve_within_workspace_root, resolve_writable_path,
};
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub const PARALLEL_LIMIT_CAP: usize = 8;

/// How a tool participates in batch conflict detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolConflictClass {
    /// No resource key (e.g. web_search).
    None,
    /// File path read/write/edit/grep/glob/list.
    FilePath,
    /// Sidecar tools — P1 stays serial.
    Sidecar,
    /// Desktop / Computer tools — force serial batch.
    Computer,
    /// `run_subagent` — limited by subagent semaphore.
    SubAgent,
    /// `image_generate` / `video_generate` / `media_understand`.
    Media,
    /// `terminal` — parallel by default (no conversation mutex).
    Terminal,
    /// `read_lints`, `cron_job`, `skill_import`, …
    SerialOnly,
}

/// Default CPU-based parallel slot count: min(logical cores, 8), at least 1.
pub fn default_parallel_limit() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .clamp(1, PARALLEL_LIMIT_CAP)
}

/// Infer registry parallel metadata from tool id and sidecar flag.
pub fn infer_parallel_metadata(name: &str, is_sidecar: bool) -> (bool, ToolConflictClass) {
    if is_sidecar {
        return (false, ToolConflictClass::Sidecar);
    }
    if desktop_tool_family_id(name).is_some()
        || matches!(
            name,
            "hotkey" | "wait" | "launch_app" | "clipboard_read" | "clipboard_write"
        )
    {
        return (false, ToolConflictClass::Computer);
    }
    match name {
        "file_read" | "file_write" | "file_edit" | "file_grep" | "file_glob" | "file_list" => {
            (true, ToolConflictClass::FilePath)
        }
        "terminal" => (true, ToolConflictClass::Terminal),
        "run_subagent" => (true, ToolConflictClass::SubAgent),
        "image_generate" | "video_generate" | "media_understand" => {
            (true, ToolConflictClass::Media)
        }
        "web_search" | "web_fetch" | "skill_read" | "session_search" | "memory" => {
            (true, ToolConflictClass::None)
        }
        "read_lints" | "cron_job" | "skill_import" | "job" => (false, ToolConflictClass::SerialOnly),
        n if n.starts_with("task_board_") => (false, ToolConflictClass::Sidecar),
        n if n.starts_with("skill_") && n != "skill_read" => (false, ToolConflictClass::SerialOnly),
        _ => (false, ToolConflictClass::SerialOnly),
    }
}

fn conflict_resolved_file_path(root: &Path, path_str: &str) -> Option<PathBuf> {
    let trimmed = path_str.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Prefer writable resolution so relative paths stay under the conversation workspace
    // (including nested tempdirs) and absolute/`~` write targets share one key space.
    if let Ok(p) = resolve_writable_path(root, trimmed) {
        return Some(p);
    }
    if let Ok(p) = resolve_accessible_path(root, trimmed) {
        return Some(p);
    }
    None
}

fn path_arg_from_file_tool(args: &Value) -> Option<String> {
    if let Some(p) = args
        .get("path")
        .or_else(|| args.get("file"))
        .or_else(|| args.get("Path"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return Some(p.to_string());
    }
    None
}

/// Resource keys for conflict detection within one assistant tool batch.
pub fn conflict_keys_for_invocation(
    tool_id: &str,
    args: &Value,
    workspace_root: &str,
    conversation_id: &str,
) -> HashSet<String> {
    let mut keys = HashSet::new();
    let root = Path::new(workspace_root.trim());
    let push_file = |keys: &mut HashSet<String>, path_str: &str| {
        if let Some(p) = conflict_resolved_file_path(root, path_str) {
            keys.insert(format!("file:{}", p.display()));
        }
    };

    match tool_id {
        "file_read" | "file_write" | "file_edit" => {
            if let Some(p) = path_arg_from_file_tool(args) {
                push_file(&mut keys, &p);
            }
        }
        "file_grep" | "file_glob" | "file_list" => {
            let path_key = args
                .get("path")
                .or_else(|| args.get("Path"))
                .or_else(|| args.get("glob"))
                .or_else(|| args.get("pattern"))
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            if let Some(p) = conflict_resolved_file_path(root, path_key)
                .or_else(|| resolve_within_workspace_root(root, path_key).ok())
            {
                keys.insert(format!("dir:{}", p.display()));
            }
        }
        "run_subagent" => {
            keys.insert(format!("subagent_slot:{conversation_id}"));
        }
        "image_generate" | "video_generate" | "media_understand" => {}
        _ => {}
    }
    keys
}

pub fn keys_overlap(a: &HashSet<String>, b: &HashSet<String>) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a.iter().any(|k| b.contains(k))
}

pub fn resolve_parallel_limit(override_value: Option<u32>) -> usize {
    override_value
        .map(|n| n.max(1) as usize)
        .unwrap_or_else(default_parallel_limit)
}

#[derive(Debug, Clone, Copy)]
pub struct ParallelLimits {
    pub max_parallel_tools: usize,
    pub max_parallel_sub_agents: usize,
    pub max_parallel_media_jobs: usize,
}

impl ParallelLimits {
    pub fn from_settings(settings: &crate::models::ModelSettings) -> Self {
        Self {
            max_parallel_tools: resolve_parallel_limit(settings.max_parallel_tool_calls),
            max_parallel_sub_agents: resolve_parallel_limit(settings.max_parallel_sub_agents),
            max_parallel_media_jobs: resolve_parallel_limit(settings.max_parallel_media_jobs),
        }
    }

    pub fn log_startup(&self, settings: &crate::models::ModelSettings) {
        let cpu = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        let source = if settings.max_parallel_tool_calls.is_some()
            || settings.max_parallel_sub_agents.is_some()
            || settings.max_parallel_media_jobs.is_some()
        {
            "settings_override"
        } else {
            "cpu_cores"
        };
        log::info!(
            "parallel_limit: tools={} subagents={} media={} cpu_cores={} cap={} source={source}",
            self.max_parallel_tools,
            self.max_parallel_sub_agents,
            self.max_parallel_media_jobs,
            cpu,
            PARALLEL_LIMIT_CAP
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn default_parallel_limit_is_capped() {
        let n = default_parallel_limit();
        assert!(n >= 1);
        assert!(n <= PARALLEL_LIMIT_CAP);
    }

    #[test]
    fn infer_parallel_metadata_file_read() {
        let (eligible, class) = infer_parallel_metadata("file_read", false);
        assert!(eligible);
        assert_eq!(class, ToolConflictClass::FilePath);
    }

    #[test]
    fn infer_parallel_metadata_sidecar() {
        let (eligible, class) = infer_parallel_metadata("task_board_patch", true);
        assert!(!eligible);
        assert_eq!(class, ToolConflictClass::Sidecar);
    }

    #[test]
    fn infer_parallel_metadata_computer() {
        let (eligible, class) = infer_parallel_metadata("mouse_click_index", false);
        assert!(!eligible);
        assert_eq!(class, ToolConflictClass::Computer);
    }

    #[test]
    fn conflict_keys_same_file_overlap() {
        let dir = tempdir().unwrap();
        let root = dir.path().to_string_lossy().into_owned();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        let args_a = serde_json::json!({"path": "a.txt"});
        let args_b = serde_json::json!({"path": "a.txt"});
        let ka = conflict_keys_for_invocation("file_write", &args_a, &root, "c1");
        let kb = conflict_keys_for_invocation("file_read", &args_b, &root, "c1");
        assert!(!ka.is_empty());
        assert!(keys_overlap(&ka, &kb));
    }

    #[test]
    fn conflict_keys_edit_overlaps_write_same_path() {
        let dir = tempdir().unwrap();
        let root = dir.path().to_string_lossy().into_owned();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        let ka = conflict_keys_for_invocation(
            "file_write",
            &serde_json::json!({"path": "a.txt"}),
            &root,
            "c1",
        );
        let kb = conflict_keys_for_invocation(
            "file_edit",
            &serde_json::json!({
                "path": "a.txt",
                "oldString": "x",
                "newString": "y"
            }),
            &root,
            "c1",
        );
        assert!(keys_overlap(&ka, &kb));
    }

    #[test]
    fn conflict_keys_different_files_no_overlap() {
        let dir = tempdir().unwrap();
        let root = dir.path().to_string_lossy().into_owned();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        std::fs::write(dir.path().join("b.txt"), "y").unwrap();
        let ka = conflict_keys_for_invocation(
            "file_read",
            &serde_json::json!({"path": "a.txt"}),
            &root,
            "c1",
        );
        let kb = conflict_keys_for_invocation(
            "file_read",
            &serde_json::json!({"path": "b.txt"}),
            &root,
            "c1",
        );
        assert!(!keys_overlap(&ka, &kb));
    }

    #[test]
    fn conflict_keys_media_understand_calls_do_not_overlap() {
        let args_a = serde_json::json!({"mode": "image", "refs": ["a.png"], "goal": "a"});
        let args_b = serde_json::json!({"mode": "image", "refs": ["b.png"], "goal": "b"});
        let ka = conflict_keys_for_invocation("media_understand", &args_a, ".", "c1");
        let kb = conflict_keys_for_invocation("media_understand", &args_b, ".", "c1");
        assert!(ka.is_empty());
        assert!(kb.is_empty());
        assert!(!keys_overlap(&ka, &kb));
    }
}
