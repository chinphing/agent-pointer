use super::path::{path_display_abs, resolve_existing_read_path};
use super::{MAX_LIST_ENTRIES, MAX_WALK_DEPTH};
use anyhow::{anyhow, Result};
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

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
    let max_results = args
        .get("maxResults")
        .or_else(|| args.get("max_results"))
        .and_then(|v| v.as_u64())
        .unwrap_or(100_u64)
        .min(MAX_LIST_ENTRIES as u64) as usize;
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
    if !base.is_dir() {
        return Err(anyhow!("不是目录: {}", base.display()));
    }
    let base_canon = base
        .canonicalize()
        .map_err(|e| anyhow!("无法解析目录: {e}"))?;

    let mut entries: Vec<serde_json::Value> = Vec::new();
    let mut truncated = false;

    if !recursive {
        for entry in fs::read_dir(&base_canon).map_err(|e| anyhow!("读取目录失败: {e}"))? {
            if entries.len() >= max_results {
                truncated = true;
                break;
            }
            let entry = entry.map_err(|e| anyhow!("{e}"))?;
            let p = entry.path();
            let meta = entry.metadata().map_err(|e| anyhow!("{e}"))?;
            let is_dir = meta.is_dir();
            if !list_entry_type_allowed(is_dir, type_filter) {
                continue;
            }
            let path_abs = path_display_abs(&p);
            entries.push(serde_json::json!({
                "path": path_abs,
                "kind": if is_dir { "directory" } else { "file" }
            }));
        }
    } else {
        let walk_cap = max_depth.max(1);
        for wd in WalkDir::new(&base_canon)
            .max_depth(walk_cap)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entries.len() >= max_results {
                truncated = true;
                break;
            }
            if wd.depth() == 0 {
                continue;
            }
            let p = wd.path();
            let is_dir = p.is_dir();
            if !p.is_file() && !is_dir {
                continue;
            }
            if !list_entry_type_allowed(is_dir, type_filter) {
                continue;
            }
            let path_abs = path_display_abs(p);
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
