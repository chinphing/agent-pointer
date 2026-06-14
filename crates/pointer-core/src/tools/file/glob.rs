use super::list::list_entry_type_allowed;
use super::path::{
    path_display_abs, resolve_existing_read_path,
};
use super::{MAX_GLOB_RESULTS, MAX_WALK_DEPTH};
use anyhow::{anyhow, Result};
use globset::{Glob, GlobSetBuilder};
use log::info;
use std::path::Path;
use walkdir::WalkDir;

fn path_is_inside_git_metadata_tree(p: &Path) -> bool {
    p.to_string_lossy()
        .replace('\\', "/")
        .contains("/.git/")
}

fn parse_glob_entry_type(args: &serde_json::Value) -> Result<&'static str> {
    let s = args
        .get("entryType")
        .or_else(|| args.get("entry_type"))
        .and_then(|v| v.as_str())
        .unwrap_or("file")
        .trim();
    match s.to_ascii_lowercase().as_str() {
        "file" | "files" => Ok("file"),
        "dir" | "directory" | "directories" => Ok("dir"),
        "all" => Ok("all"),
        _ => Err(anyhow!(
            "无效的 glob entryType（允许 file | dir | all）: {s}"
        )),
    }
}

pub(crate) fn execute_file_glob_payload(args: &serde_json::Value, root: &Path) -> Result<String> {
    let pattern = args
        .get("pattern")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("缺少 pattern"))?;
    let max_results = args
        .get("maxResults")
        .and_then(|v| v.as_u64())
        .unwrap_or(MAX_GLOB_RESULTS as u64)
        .min(MAX_GLOB_RESULTS as u64) as usize;
    let max_depth = args
        .get("maxDepth")
        .and_then(|v| v.as_u64())
        .unwrap_or(MAX_WALK_DEPTH as u64)
        .min(MAX_WALK_DEPTH as u64) as usize;
    let entry_type = parse_glob_entry_type(args)?;
    let include_hidden = args
        .get("includeHidden")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let walk_root = if let Some(b) = args
        .get("base")
        .or_else(|| args.get("rootPath"))
        .or_else(|| args.get("baseDir"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let p = resolve_existing_read_path(root, b, "glob")?;
        if !p.is_dir() {
            return Err(anyhow!("glob 搜索根必须是目录: {}", p.display()));
        }
        p
    } else {
        root.to_path_buf()
    };
    let walk_root = walk_root
        .canonicalize()
        .map_err(|e| anyhow!("glob 搜索根路径无效: {e}"))?;

    let glob = Glob::new(pattern).map_err(|e| anyhow!("glob 模式无效: {e}"))?;
    let mut builder = GlobSetBuilder::new();
    builder.add(glob);
    let set = builder.build().map_err(|e| anyhow!("glob 构建失败: {e}"))?;

    let mut matches = Vec::new();
    let mut truncated = false;
    let walker = WalkDir::new(&walk_root)
        .max_depth(max_depth)
        .into_iter()
        .filter_entry(|e| {
            if path_is_inside_git_metadata_tree(e.path()) {
                return false;
            }
            if !include_hidden && e.depth() > 0 {
                if e.file_name().to_string_lossy().starts_with('.') {
                    return false;
                }
            }
            true
        })
        .filter_map(|e| e.ok());

    for entry in walker {
        if matches.len() >= max_results {
            truncated = true;
            break;
        }
        let p = entry.path();
        let is_dir = p.is_dir();
        if !list_entry_type_allowed(is_dir, entry_type) {
            continue;
        }
        let Ok(canon_p) = p.canonicalize() else {
            continue;
        };
        if !canon_p.starts_with(&walk_root) {
            continue;
        }
        let rel_to_walk = match canon_p.strip_prefix(&walk_root) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let mut rel_to_walk_norm = rel_to_walk.to_string_lossy().replace('\\', "/");
        if rel_to_walk_norm.is_empty() {
            rel_to_walk_norm = ".".into();
        }
        if !set.is_match(Path::new(rel_to_walk_norm.as_str())) {
            continue;
        }
        matches.push(path_display_abs(canon_p.as_path()));
    }

    info!(
        "file:glob pattern={} entryType={} includeHidden={} count={}",
        pattern,
        entry_type,
        include_hidden,
        matches.len()
    );

    Ok(serde_json::json!({
        "root": walk_root.display().to_string(),
        "pattern": pattern,
        "entryType": entry_type,
        "includeHidden": include_hidden,
        "matches": matches,
        "count": matches.len(),
        "truncated": truncated
    })
    .to_string())
}
