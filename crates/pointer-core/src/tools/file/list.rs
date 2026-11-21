use super::path::{path_display_abs, resolve_existing_read_path};
use super::tool_env;
use super::{json_u64_opt_keys, DEFAULT_LIST_ENTRIES, MAX_LIST_ENTRIES, MAX_WALK_DEPTH};
use crate::env::WalkOptions;
use anyhow::{anyhow, Result};
use log::{info, warn};
use std::path::Path;

pub(crate) fn list_entry_type_allowed(is_dir: bool, type_filter: &str) -> bool {
    match type_filter.trim().to_ascii_lowercase().as_str() {
        "all" => true,
        "file" | "files" => !is_dir,
        "dir" | "directory" | "directories" => is_dir,
        _ => true,
    }
}

pub(crate) fn execute_file_list_payload(args: &serde_json::Value, root: &Path) -> Result<String> {
    let path_str = args
        .get("path")
        .or_else(|| args.get("directory"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("缺少 path 或 directory（要列出的目录）"))?;

    let recursive = args
        .get("recursive")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let ceiling = MAX_LIST_ENTRIES as u64;
    let requested = json_u64_opt_keys(args, &["limit"]).map(|(n, _)| n.max(1));
    if requested.is_some_and(|n| n > ceiling) {
        warn!(
            "file_list: limit={} exceeds ceiling {}, clamping",
            requested.unwrap(),
            ceiling
        );
    }
    let max_results = requested
        .unwrap_or(DEFAULT_LIST_ENTRIES as u64)
        .min(ceiling) as usize;
    info!("file_list: limit={max_results} recursive={recursive}");
    let max_depth = args
        .get("maxDepth")
        .or_else(|| args.get("max_depth"))
        .and_then(|v| v.as_u64())
        .unwrap_or(if recursive { 2 } else { 1 })
        .min(MAX_WALK_DEPTH as u64) as usize;

    let type_filter = args
        .get("entryType")
        .or_else(|| args.get("entry_type"))
        .and_then(|v| v.as_str())
        .unwrap_or("all");

    let base = resolve_existing_read_path(root, path_str, "list")?;
    let env = tool_env(root)?;
    if !env.metadata(&base).map(|m| m.is_dir).unwrap_or(false) {
        return Err(anyhow!("不是目录: {}", base.display()));
    }
    let base_canon = env
        .canonicalize(&base)
        .map_err(|e| anyhow!("无法解析目录: {e}"))?;

    let mut entries: Vec<serde_json::Value> = Vec::new();
    let mut truncated = false;

    if !recursive {
        for entry in env
            .list_dir(&base_canon)
            .map_err(|e| anyhow!("读取目录失败: {e}"))?
        {
            if entries.len() >= max_results {
                truncated = true;
                break;
            }
            // 符号链接按目标类型判定，与旧 `entry.metadata()` 一致。
            let is_dir = if entry.is_symlink {
                env.metadata(&entry.path).map(|m| m.is_dir).unwrap_or(false)
            } else {
                entry.is_dir
            };
            if !list_entry_type_allowed(is_dir, type_filter) {
                continue;
            }
            let path_abs = path_display_abs(&entry.path);
            entries.push(serde_json::json!({
                "path": path_abs,
                "kind": if is_dir { "directory" } else { "file" }
            }));
        }
    } else {
        let walk_cap = max_depth.max(1);
        // `file_list` 的语义是"这里有什么"：隐藏项与 `.git` 都要列出来（与旧 walkdir 行为一致），
        // 只有 grep / glob 才按 hidden / gitignore 过滤。
        for wd in env
            .walk(
                &base_canon,
                WalkOptions::new(walk_cap)
                    .with_hidden(true)
                    .with_git_metadata(false),
            )
            .map_err(|e| anyhow!("{e}"))?
        {
            if entries.len() >= max_results {
                truncated = true;
                break;
            }
            let is_dir = if wd.is_symlink {
                env.metadata(&wd.path).map(|m| m.is_dir).unwrap_or(false)
            } else {
                wd.is_dir
            };
            if !list_entry_type_allowed(is_dir, type_filter) {
                continue;
            }
            let path_abs = path_display_abs(&wd.path);
            entries.push(serde_json::json!({
                "path": path_abs,
                "kind": if is_dir { "directory" } else { "file" }
            }));
        }
    }

    Ok(serde_json::json!({
        "directory": base_canon.display().to_string(),
        "recursive": recursive,
        "maxDepth": max_depth,
        "entryType": type_filter,
        "entries": entries,
        "count": entries.len(),
        "truncated": truncated
    })
    .to_string())
}
