use super::list::list_entry_type_allowed;
use super::path::{
    expand_user_path_for_file, path_display_abs, resolve_existing_read_path,
};
use super::{MAX_GLOB_RESULTS, MAX_WALK_DEPTH};
use anyhow::{anyhow, Result};
use globset::{Glob, GlobSetBuilder};
use log::info;
use std::path::{Component, Path, PathBuf};
use walkdir::WalkDir;

fn path_is_inside_git_metadata_tree(p: &Path) -> bool {
    p.to_string_lossy()
        .replace('\\', "/")
        .contains("/.git/")
}

fn has_glob_meta(s: &str) -> bool {
    s.chars().any(|c| matches!(c, '*' | '?' | '[' | '{'))
}

/// `**/*.ext` does not match `file.ext` at the walk root in globset; add a sibling pattern.
fn build_glob_set(pattern: &str) -> Result<globset::GlobSet> {
    let mut builder = GlobSetBuilder::new();
    builder.add(Glob::new(pattern).map_err(|e| anyhow!("glob 模式无效: {e}"))?);
    if let Some(rest) = pattern.strip_prefix("**/") {
        builder.add(Glob::new(rest).map_err(|e| anyhow!("glob 模式无效: {e}"))?);
    } else if let Some(i) = pattern.find("/**/") {
        let alt = format!("{}{}{}", &pattern[..i], "/", &pattern[i + 4..]);
        builder.add(Glob::new(&alt).map_err(|e| anyhow!("glob 模式无效: {e}"))?);
    }
    builder
        .build()
        .map_err(|e| anyhow!("glob 构建失败: {e}"))
}

/// When `pattern` is absolute / `~/…`, split into (search root, relative glob).
fn split_absolute_glob_pattern(pattern: &str) -> Result<Option<(PathBuf, String)>> {
    let trimmed = pattern.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let expanded = expand_user_path_for_file(trimmed)?;
    let path = PathBuf::from(&expanded);
    if !path.is_absolute() {
        return Ok(None);
    }

    let mut literal = PathBuf::new();
    let mut glob_parts: Vec<String> = Vec::new();
    let mut in_glob = false;
    for comp in path.components() {
        match comp {
            Component::Prefix(p) => {
                if !in_glob {
                    literal.push(p.as_os_str());
                }
            }
            Component::RootDir => {
                if !in_glob {
                    literal.push(comp.as_os_str());
                }
            }
            Component::Normal(c) => {
                let s = c.to_string_lossy();
                if !in_glob && !has_glob_meta(&s) {
                    literal.push(c);
                } else {
                    in_glob = true;
                    glob_parts.push(s.into_owned());
                }
            }
            Component::CurDir | Component::ParentDir => {
                if !in_glob {
                    literal.push(comp.as_os_str());
                } else {
                    glob_parts.push(comp.as_os_str().to_string_lossy().into_owned());
                }
            }
        }
    }

    let rel = if glob_parts.is_empty() {
        "*".to_string()
    } else {
        glob_parts.join("/")
    };

    let mut walk = literal;
    while !walk.as_os_str().is_empty() && !walk.is_dir() {
        if !walk.pop() {
            break;
        }
    }
    if !walk.is_dir() {
        return Err(anyhow!(
            "glob pattern 绝对路径前缀不存在或不是目录: {expanded}"
        ));
    }
    Ok(Some((walk, rel)))
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
    let pattern_raw = args
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

    let (walk_root, pattern) = if let Some((abs_root, rel)) = split_absolute_glob_pattern(pattern_raw)?
    {
        (abs_root, rel)
    } else {
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
        (walk_root, pattern_raw.to_string())
    };
    let walk_root = walk_root
        .canonicalize()
        .map_err(|e| anyhow!("glob 搜索根路径无效: {e}"))?;

    let set = build_glob_set(&pattern)?;

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
