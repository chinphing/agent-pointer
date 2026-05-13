//! Workspace-scoped file tools: single registry tool `file` with `method` (like Computer `mouse:method`).
//! Root from settings `workspaceRoot`, else `current_dir`.
use super::{ToolEntry, ToolHandler, ToolRegistry};
use crate::storage;
use anyhow::{anyhow, Result};
use log::warn;
use globset::{Glob, GlobSetBuilder};
use regex::RegexBuilder;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use walkdir::WalkDir;

/// Doc for registry tool `file`; keep in sync with `prompts/file.md`.
const FILE_MD: &str = include_str!("prompts/file.md");

const MAX_FILE_READ_BYTES: usize = 256 * 1024;
/// Max files per `file` read batch (`paths`). **Keep in sync** with `prompts/file.md` Parameters section.
const MAX_FILE_READ_BATCH: usize = 32;
/// Default cap on combined UTF-8 length of all `content` fields in one `paths` batch (assistant context).
/// **Keep in sync** with `prompts/file.md` (`maxTotalBytes`).
const MAX_FILE_READ_BATCH_TOTAL_BYTES_DEFAULT: usize = 1024 * 1024;
/// Hard upper bound for caller-supplied `maxTotalBytes`.
const MAX_FILE_READ_BATCH_TOTAL_BYTES_CLAMP: usize = 4 * 1024 * 1024;
/// Do not emit a tiny truncated slice; skip with an error instead.
const MIN_BATCH_TRUNCATE_REMAINING: usize = 256;
const MAX_GREP_RESULTS: usize = 200;
const MAX_GREP_FILE_BYTES: usize = 2 * 1024 * 1024;
const MAX_GLOB_RESULTS: usize = 500;
const MAX_LIST_ENTRIES: usize = 2000;
const MAX_WALK_DEPTH: usize = 64;
const CONTEXT_LINES: usize = 2;

fn args_without_method(args: &serde_json::Value) -> serde_json::Value {
    match args {
        serde_json::Value::Object(m) => {
            let mut m = m.clone();
            m.remove("method");
            serde_json::Value::Object(m)
        }
        _ => args.clone(),
    }
}

/// Binary-ish extensions to skip in grep
const SKIP_EXT: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "ico", "pdf", "zip", "gz", "7z", "rar", "exe", "dll",
    "so", "dylib", "wasm", "mp3", "mp4", "avi", "mkv", "ttf", "woff", "woff2", "eot",
];

pub fn register_all(reg: &ToolRegistry) {
    let doc = FILE_MD.trim();
    let h: ToolHandler = Arc::new(|args| {
        let root = resolve_tool_workspace_root()?;
        execute_file_tool(&args, &root)
    });
    reg.register(ToolEntry::new(
        "file",
        "low",
        false,
        doc.trim(),
        None,
        h,
    ));
}

/// Root for sandbox: configured workspace or current directory.
pub fn resolve_tool_workspace_root() -> Result<PathBuf> {
    let s = storage::load_settings().map_err(|e| anyhow!("读取设置失败: {e}"))?;
    let raw = s.workspace_root.trim();
    if !raw.is_empty() {
        let p = PathBuf::from(raw);
        if !p.is_dir() {
            return Err(anyhow!("工作区目录无效或不存在: {}", raw));
        }
        return p
            .canonicalize()
            .map_err(|e| anyhow!("无法解析工作区路径: {e}"));
    }
    std::env::current_dir().map_err(|e| anyhow!("无法获取当前目录: {e}"))
}

/// Resolve `user_path` (relative to root or absolute under root). Rejects traversal outside root.
pub fn resolve_within_workspace_root(root: &Path, user_path: &str) -> Result<PathBuf> {
    let root = root
        .canonicalize()
        .map_err(|e| anyhow!("工作区根无效: {e}"))?;
    let user_path = user_path.trim();
    if user_path.is_empty() {
        return Err(anyhow!("路径不能为空"));
    }
    if user_path.contains('\0') {
        return Err(anyhow!("路径含非法字符"));
    }
    let path = Path::new(user_path);

    let out = if path.is_absolute() {
        path.canonicalize()
            .map_err(|e| anyhow!("路径无效: {e}"))?
    } else {
        let mut acc = root.clone();
        for c in path.components() {
            match c {
                Component::Prefix(_) | Component::RootDir => {
                    return Err(anyhow!("相对路径含非法根组件"));
                }
                Component::CurDir => {}
                Component::ParentDir => {
                    if !acc.pop() {
                        return Err(anyhow!("路径越出工作区"));
                    }
                    if !acc.starts_with(&root) {
                        return Err(anyhow!("路径越出工作区"));
                    }
                }
                Component::Normal(s) => acc.push(s),
            }
        }
        acc
    };

    if !out.starts_with(&root) {
        return Err(anyhow!("路径不在工作区内"));
    }
    Ok(out)
}

/// Resolve paths for **read-only** `file` methods. Relative paths must stay under `workspace_root`.
/// **Absolute** paths are canonicalized as-is so other projects can be read when the user provides them.
pub fn resolve_accessible_path(workspace_root: &Path, user_path: &str) -> Result<PathBuf> {
    let workspace_root = workspace_root
        .canonicalize()
        .map_err(|e| anyhow!("工作区根无效: {e}"))?;
    let user_path = user_path.trim();
    if user_path.is_empty() {
        return Err(anyhow!("路径不能为空"));
    }
    if user_path.contains('\0') {
        return Err(anyhow!("路径含非法字符"));
    }
    let path = Path::new(user_path);
    if path.is_absolute() {
        return path
            .canonicalize()
            .map_err(|e| anyhow!("路径无效或不存在: {e}"));
    }
    resolve_within_workspace_root(&workspace_root, user_path)
}

/// Normalize line breaks to `\n` so `file_read` output (LF-joined) can match CR / CRLF on disk.
fn normalize_newlines_lf(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n")
}

/// Replace `old_s` with `new_s` at most one occurrence, requiring a unique match.
///
/// `file_read` joins logical lines with `\n` only, while Windows repos often use `\r\n` on disk.
/// If the model copies from `file_read`, exact substring match would fail on CRLF files; we try
/// LF↔CRLF variants when the primary match count is zero.
///
/// Finally we match in **LF-normalized** space (handles CR-only / mixed breaks) and, if the
/// original file contained `\r\n`, write back with `\r\n`.
fn try_unique_text_replace(text: &str, old_s: &str, new_s: &str) -> Result<String> {
    fn count_and_replace(text: &str, old: &str, new: &str) -> Result<Option<String>> {
        let c = text.matches(old).count();
        if c == 1 {
            return Ok(Some(text.replacen(old, new, 1)));
        }
        if c > 1 {
            return Err(anyhow!("oldString 匹配到 {c} 处，必须唯一"));
        }
        Ok(None)
    }

    if let Some(s) = count_and_replace(text, old_s, new_s)? {
        return Ok(s);
    }

    // Model used `\n` between lines (e.g. from file_read); file may be CRLF.
    if !old_s.contains('\r') {
        let old_crlf = old_s.replace('\n', "\r\n");
        let new_crlf = new_s.replace('\n', "\r\n");
        if old_crlf != old_s {
            if let Some(s) = count_and_replace(text, &old_crlf, &new_crlf)? {
                return Ok(s);
            }
        }
    }

    // Model used CRLF; file may be LF-only.
    if old_s.contains("\r\n") {
        let old_lf = old_s.replace("\r\n", "\n");
        let new_lf = new_s.replace("\r\n", "\n");
        if old_lf != old_s {
            if let Some(s) = count_and_replace(text, &old_lf, &new_lf)? {
                return Ok(s);
            }
        }
    }

    // Last resort: compare after normalizing all line endings to `\n` (CR-only files, odd mixes).
    let text_lf = normalize_newlines_lf(text);
    let old_lf = normalize_newlines_lf(old_s);
    let new_lf = normalize_newlines_lf(new_s);
    let c = text_lf.matches(old_lf.as_str()).count();
    if c == 1 {
        let out_lf = text_lf.replacen(&old_lf, &new_lf, 1);
        let prefer_crlf = text.contains("\r\n");
        let out = if prefer_crlf {
            out_lf.replace('\n', "\r\n")
        } else {
            out_lf
        };
        return Ok(out);
    }
    if c > 1 {
        return Err(anyhow!("oldString 匹配到 {c} 处（按换行规范化后），必须唯一"));
    }

    let preview: String = old_s.chars().take(120).collect();
    let ellipsis = if old_s.chars().count() > 120 { "…" } else { "" };
    Err(anyhow!(
        "未找到匹配的 oldString。请从本工具 file:read 或 file:grep 复制原文（含缩进），并包含足够上下文保证唯一；注意模型输出可能合并空格/省略片段。当前 oldString 前 120 字符：{}{}",
        preview,
        ellipsis
    ))
}

/// Read one UTF-8 text file within workspace; returns JSON object for success or `{ path, error }`.
fn file_read_one_json(
    root: &Path,
    path_str: &str,
    line_start: usize,
    line_end_exclusive: Option<usize>,
    max_bytes: usize,
) -> serde_json::Value {
    let full = match resolve_accessible_path(root, path_str) {
        Ok(p) => p,
        Err(e) => {
            return serde_json::json!({
                "path": path_str,
                "error": e.to_string(),
            });
        }
    };
    let full_display = full.display().to_string();
    if !full.is_file() {
        return serde_json::json!({
            "path": full_display,
            "error": format!("不是文件: {}", full.display()),
        });
    }
    let meta = match fs::metadata(&full) {
        Ok(m) => m,
        Err(e) => {
            return serde_json::json!({
                "path": full_display,
                "error": format!("读取元数据失败: {e}"),
            });
        }
    };
    if meta.len() > max_bytes as u64 {
        return serde_json::json!({
            "path": full_display,
            "error": format!(
                "文件过大 ({} bytes)，超过上限 {}。请缩小范围或使用 grep。",
                meta.len(),
                max_bytes
            ),
        });
    }
    let bytes = match fs::read(&full) {
        Ok(b) => b,
        Err(e) => {
            return serde_json::json!({
                "path": full_display,
                "error": format!("读取文件失败: {e}"),
            });
        }
    };
    let text = match String::from_utf8(bytes) {
        Ok(t) => t,
        Err(_) => {
            return serde_json::json!({
                "path": full_display,
                "error": "非 UTF-8 文本，无法作为文本读取",
            });
        }
    };
    let lines: Vec<&str> = text.lines().collect();
    let start_idx = line_start.saturating_sub(1).min(lines.len());
    let end_idx = if let Some(le) = line_end_exclusive {
        le.saturating_sub(1).min(lines.len()).max(start_idx)
    } else {
        lines.len()
    };
    let slice = &lines[start_idx..end_idx];
    let content = slice.join("\n");
    serde_json::json!({
        "path": full_display,
        "lineStart": line_start,
        "lineEndExclusive": line_end_exclusive.unwrap_or(lines.len() + 1),
        "totalLines": lines.len(),
        "content": content,
        "truncated": false,
    })
}

/// Returns a prefix of `s` whose UTF-8 byte length does not exceed `max_bytes`.
fn utf8_byte_prefix(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut n = max_bytes;
    while n > 0 && !s.is_char_boundary(n) {
        n -= 1;
    }
    &s[..n]
}

/// Core logic for `file_read` (single `path` or batch `paths`). Used by tests with an explicit root.
fn execute_file_read(args: &serde_json::Value, root: &Path) -> Result<String> {
    let line_start = args
        .get("lineStart")
        .and_then(|v| v.as_u64())
        .unwrap_or(1)
        .max(1) as usize;
    // lineEnd: 1-based exclusive (与 Rust range 一致：读到「该行之前」)。缺省读到文件末尾。
    let line_end_exclusive = args
        .get("lineEnd")
        .and_then(|v| v.as_u64())
        .map(|n| n.max(1) as usize);
    let max_bytes = args
        .get("maxBytes")
        .and_then(|v| v.as_u64())
        .unwrap_or(MAX_FILE_READ_BYTES as u64)
        .min(MAX_FILE_READ_BYTES as u64) as usize;

    let paths_from_array: Option<Vec<String>> = match args.get("paths") {
        None => None,
        Some(v) => {
            let arr = v
                .as_array()
                .ok_or_else(|| anyhow!("paths 须为字符串数组"))?;
            Some(
                arr.iter()
                    .filter_map(|x| x.as_str().map(str::trim))
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect(),
            )
        }
    };

    let use_batch = paths_from_array
        .as_ref()
        .map(|p| !p.is_empty())
        .unwrap_or(false);

    if use_batch {
        let paths = paths_from_array.unwrap_or_default();
        if paths.len() > MAX_FILE_READ_BATCH {
            return Err(anyhow!(
                "一次最多读取 {} 个文件（当前 {}）",
                MAX_FILE_READ_BATCH,
                paths.len()
            ));
        }
        let max_total_bytes = args
            .get("maxTotalBytes")
            .and_then(|v| v.as_u64())
            .map(|n| {
                (n as usize)
                    .max(1)
                    .min(MAX_FILE_READ_BATCH_TOTAL_BYTES_CLAMP)
            })
            .unwrap_or(MAX_FILE_READ_BATCH_TOTAL_BYTES_DEFAULT);

        let mut content_bytes: usize = 0;
        let mut batch_capped = false;
        let mut budget_done = false;
        let mut files: Vec<serde_json::Value> = Vec::with_capacity(paths.len());

        for p in &paths {
            if budget_done || content_bytes >= max_total_bytes {
                files.push(serde_json::json!({
                    "path": p,
                    "error": "未读取：本批正文已达 maxTotalBytes 上限。请减少 paths、使用 lineStart/lineEnd、降低 maxBytes，或拆成多次 file:read。",
                }));
                batch_capped = true;
                continue;
            }

            let mut v = file_read_one_json(root, p, line_start, line_end_exclusive, max_bytes);

            if v.get("error").is_some() {
                files.push(v);
                continue;
            }

            let Some(content) = v.get("content").and_then(|c| c.as_str()) else {
                files.push(v);
                continue;
            };

            let next_total = content_bytes.saturating_add(content.len());
            if next_total <= max_total_bytes {
                content_bytes = next_total;
                files.push(v);
                continue;
            }

            let budget = max_total_bytes.saturating_sub(content_bytes);
            if budget < MIN_BATCH_TRUNCATE_REMAINING {
                files.push(serde_json::json!({
                    "path": p,
                    "error": format!(
                        "本批剩余空间过小（{} 字节），无法容纳此文件正文。请提高 maxTotalBytes、减少 paths，或改用 lineStart/lineEnd。",
                        budget
                    ),
                }));
                batch_capped = true;
                budget_done = true;
                continue;
            }

            let tail = "\n…[已截断：达到本批 maxTotalBytes]";
            let prefix_budget = budget.saturating_sub(tail.len());
            let prefix = utf8_byte_prefix(content, prefix_budget);
            let new_content = format!("{prefix}{tail}");

            if let serde_json::Value::Object(ref mut m) = v {
                m.insert("content".to_string(), serde_json::json!(new_content));
                m.insert("truncated".to_string(), serde_json::json!(true));
                m.insert("batchTruncated".to_string(), serde_json::json!(true));
            }

            content_bytes += new_content.len();
            batch_capped = true;
            budget_done = true;
            files.push(v);
        }

        if batch_capped {
            warn!(
                "file:read batch hit maxTotalBytes={}; returned {} file entries (truncated and/or skipped)",
                max_total_bytes,
                files.len()
            );
        }

        return Ok(serde_json::json!({
            "files": files,
            "maxTotalBytes": max_total_bytes,
            "contentBytes": content_bytes,
            "batchCapped": batch_capped,
        })
        .to_string());
    }

    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("缺少 path；批量读取请传 paths 数组"))?;

    let v = file_read_one_json(root, path, line_start, line_end_exclusive, max_bytes);
    if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
        return Err(anyhow!("{}", err));
    }
    Ok(v.to_string())
}

fn execute_file_tool(args: &serde_json::Value, root: &Path) -> Result<String> {
    let method = args
        .get("method")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("缺少 method；或使用限定名 file:read / file:write / file:edit / file:glob / file:grep"))?;
    let payload = args_without_method(args);
    match method {
        "read" => execute_file_read(&payload, root),
        "write" => execute_file_write_payload(&payload, root),
        "edit" => execute_file_edit_payload(&payload, root),
        "glob" => execute_file_glob_payload(&payload, root),
        "grep" => execute_file_grep_payload(&payload, root),
        "list" => execute_file_list_payload(&payload, root),
        _ => Err(anyhow!(
            "未知 file.method: {method}（允许 read | write | edit | glob | grep | list）"
        )),
    }
}

fn execute_file_write_payload(args: &serde_json::Value, root: &Path) -> Result<String> {
    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("缺少 path"))?;
    let content = args
        .get("content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("缺少 content"))?;

    let full = resolve_within_workspace_root(root, path)?;
    if let Some(parent) = full.parent() {
        fs::create_dir_all(parent).map_err(|e| anyhow!("创建目录失败: {e}"))?;
    }
    fs::write(&full, content.as_bytes()).map_err(|e| anyhow!("写入失败: {e}"))?;
    Ok(serde_json::json!({
        "path": full.display().to_string(),
        "bytesWritten": content.as_bytes().len(),
        "success": true
    })
    .to_string())
}

/// Tool JSON often uses camelCase in schema; models trained on other agents may emit snake_case.
fn json_str<'a>(args: &'a serde_json::Value, camel: &str, snake: &str) -> Option<&'a str> {
    args.get(camel)
        .and_then(|v| v.as_str())
        .or_else(|| args.get(snake).and_then(|v| v.as_str()))
}

fn execute_file_edit_payload(args: &serde_json::Value, root: &Path) -> Result<String> {
    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("缺少 path"))?;
    let old_s = json_str(args, "oldString", "old_string")
        .ok_or_else(|| anyhow!("缺少 oldString（或 old_string）"))?;
    let new_s = json_str(args, "newString", "new_string")
        .ok_or_else(|| anyhow!("缺少 newString（或 new_string）"))?;
    if old_s.is_empty() {
        return Err(anyhow!("oldString 不能为空"));
    }

    let full = resolve_within_workspace_root(root, path)?;
    if !full.is_file() {
        return Err(anyhow!("不是文件: {}", full.display()));
    }
    let text = fs::read_to_string(&full).map_err(|e| anyhow!("读取失败: {e}"))?;
    let updated = try_unique_text_replace(&text, old_s, new_s)?;
    fs::write(&full, updated.as_bytes()).map_err(|e| anyhow!("写入失败: {e}"))?;
    Ok(serde_json::json!({
        "path": full.display().to_string(),
        "replaced": 1,
        "success": true
    })
    .to_string())
}

fn list_entry_type_allowed(is_dir: bool, type_filter: &str) -> bool {
    match type_filter.trim().to_ascii_lowercase().as_str() {
        "all" => true,
        "file" | "files" => !is_dir,
        "dir" | "directory" | "directories" => is_dir,
        _ => true,
    }
}

fn execute_file_list_payload(args: &serde_json::Value, root: &Path) -> Result<String> {
    let path_str = args
        .get("path")
        .or_else(|| args.get("directory"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("缺少 path 或 directory（要列出的目录）"))?;

    let recursive = args.get("recursive").and_then(|v| v.as_bool()).unwrap_or(false);
    let max_depth = args
        .get("maxDepth")
        .or_else(|| args.get("max_depth"))
        .and_then(|v| v.as_u64())
        .unwrap_or(if recursive { 8 } else { 1 })
        .min(MAX_WALK_DEPTH as u64) as usize;

    let type_filter = args
        .get("entryType")
        .or_else(|| args.get("entry_type"))
        .and_then(|v| v.as_str())
        .unwrap_or("all");

    let base = resolve_accessible_path(root, path_str)?;
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
            if entries.len() >= MAX_LIST_ENTRIES {
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
            let rel = p.strip_prefix(&base_canon).unwrap_or(&p);
            let rel_s = rel.to_string_lossy().replace('\\', "/");
            entries.push(serde_json::json!({
                "path": rel_s,
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
            if entries.len() >= MAX_LIST_ENTRIES {
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
            let rel = p.strip_prefix(&base_canon).unwrap_or(p);
            let rel_s = rel.to_string_lossy().replace('\\', "/");
            entries.push(serde_json::json!({
                "path": rel_s,
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

fn execute_file_glob_payload(args: &serde_json::Value, root: &Path) -> Result<String> {
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

    let walk_root = if let Some(b) = args
        .get("base")
        .or_else(|| args.get("rootPath"))
        .or_else(|| args.get("baseDir"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let p = resolve_accessible_path(root, b)?;
        if !p.is_dir() {
            return Err(anyhow!("glob 搜索根必须是目录: {}", p.display()));
        }
        p
    } else {
        root.to_path_buf()
    };

    let glob = Glob::new(pattern).map_err(|e| anyhow!("glob 模式无效: {e}"))?;
    let mut builder = GlobSetBuilder::new();
    builder.add(glob);
    let set = builder.build().map_err(|e| anyhow!("glob 构建失败: {e}"))?;

    let mut matches = Vec::new();
    for entry in WalkDir::new(&walk_root)
        .max_depth(max_depth)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if matches.len() >= max_results {
            break;
        }
        let p = entry.path();
        if p.is_file() {
            let rel = p.strip_prefix(&walk_root).unwrap_or(p);
            let rel_norm = rel.to_string_lossy().replace('\\', "/");
            if set.is_match(Path::new(&rel_norm)) {
                matches.push(rel_norm);
            }
        }
    }
    Ok(serde_json::json!({
        "root": walk_root.display().to_string(),
        "pattern": pattern,
        "matches": matches,
        "count": matches.len(),
        "truncated": matches.len() >= max_results
    })
    .to_string())
}

fn should_skip_grep(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.rsplit_once('.'))
        .map(|(_, ext)| SKIP_EXT.iter().any(|s| s.eq_ignore_ascii_case(ext)))
        .unwrap_or(false)
}

fn execute_file_grep_payload(args: &serde_json::Value, root: &Path) -> Result<String> {
    let pattern = args
        .get("pattern")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("缺少 pattern"))?;
    if pattern.len() > 512 {
        return Err(anyhow!("正则过长"));
    }
    let subdir = args.get("subdir").and_then(|v| v.as_str()).unwrap_or("");
    let max_results = args
        .get("maxResults")
        .and_then(|v| v.as_u64())
        .unwrap_or(MAX_GREP_RESULTS as u64)
        .min(MAX_GREP_RESULTS as u64) as usize;
    let max_depth = args
        .get("maxDepth")
        .and_then(|v| v.as_u64())
        .unwrap_or(MAX_WALK_DEPTH as u64)
        .min(MAX_WALK_DEPTH as u64) as usize;
    let context = args
        .get("contextLines")
        .and_then(|v| v.as_u64())
        .unwrap_or(CONTEXT_LINES as u64)
        .min(5) as usize;

    let re = RegexBuilder::new(pattern)
        .multi_line(true)
        .build()
        .map_err(|e| anyhow!("正则无效: {e}"))?;

    let search_root = if subdir.trim().is_empty() {
        root.to_path_buf()
    } else {
        let p = resolve_accessible_path(root, subdir)?;
        if !p.is_dir() {
            return Err(anyhow!("grep 子目录必须是目录: {}", p.display()));
        }
        p
    };

    let mut results: Vec<serde_json::Value> = Vec::new();
    for entry in WalkDir::new(&search_root)
        .max_depth(max_depth)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if results.len() >= max_results {
            break;
        }
        let p = entry.path();
        if !p.is_file() || should_skip_grep(p) {
            continue;
        }
        let meta = match fs::metadata(p) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.len() > MAX_GREP_FILE_BYTES as u64 {
            continue;
        }
        let bytes = match fs::read(p) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let text = match String::from_utf8(bytes) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let rel = p.strip_prefix(&search_root).unwrap_or(p);
        let rel_s = rel.to_string_lossy().replace('\\', "/");
        for (line_no, line) in text.lines().enumerate() {
            if results.len() >= max_results {
                break;
            }
            let n = line_no + 1;
            if re.is_match(line) {
                let lines: Vec<&str> = text.lines().collect();
                let lo = line_no.saturating_sub(context);
                let hi = (line_no + context + 1).min(lines.len());
                let ctx = lines[lo..hi]
                    .iter()
                    .enumerate()
                    .map(|(i, l)| {
                        let num = lo + i + 1;
                        format!("{num}: {l}")
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                results.push(serde_json::json!({
                    "path": rel_s,
                    "line": n,
                    "matchLine": line,
                    "context": ctx
                }));
            }
        }
    }

    Ok(serde_json::json!({
        "root": search_root.display().to_string(),
        "pattern": pattern,
        "results": results,
        "count": results.len(),
        "truncated": results.len() >= max_results
    })
    .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;

    #[test]
    fn file_read_batch_paths_returns_files_array() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        fs::write(root.join("a.txt"), "alpha\n").unwrap();
        fs::write(root.join("b.txt"), "beta\n").unwrap();

        let args = json!({
            "paths": ["a.txt", "b.txt"],
            "lineStart": 1
        });
        let out = execute_file_read(&args, root).expect("batch read");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let files = v["files"].as_array().expect("files array");
        assert_eq!(files.len(), 2);
        assert!(files[0]["content"].as_str().unwrap().contains("alpha"));
        assert!(files[1]["content"].as_str().unwrap().contains("beta"));
        assert!(files[0].get("error").is_none());
        assert!(files[1].get("error").is_none());
        assert_eq!(v["batchCapped"], false);
        assert!(v["contentBytes"].as_u64().unwrap() > 0);
    }

    #[test]
    fn file_read_batch_partial_error_preserves_ok_entries() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        fs::write(root.join("ok.txt"), "fine\n").unwrap();

        let args = json!({
            "paths": ["ok.txt", "missing.txt"],
        });
        let out = execute_file_read(&args, root).expect("batch partial");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let files = v["files"].as_array().unwrap();
        assert_eq!(files.len(), 2);
        assert!(files[0]["content"].as_str().unwrap().contains("fine"));
        assert!(files[1]["error"].as_str().unwrap().len() > 0);
    }

    #[test]
    fn file_read_batch_truncates_when_max_total_bytes_exceeded() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        fs::write(root.join("a.txt"), "a".repeat(800)).unwrap();
        fs::write(root.join("b.txt"), "b".repeat(800)).unwrap();

        let args = json!({
            "paths": ["a.txt", "b.txt"],
            "maxTotalBytes": 1200,
            "maxBytes": 10_000,
        });
        let out = execute_file_read(&args, root).expect("batch read");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["batchCapped"], true);
        assert!(v["contentBytes"].as_u64().unwrap() <= 1200);
        assert!(v["contentBytes"].as_u64().unwrap() > 800);
        let files = v["files"].as_array().unwrap();
        assert_eq!(files[0]["content"].as_str().unwrap().len(), 800);
        assert!(files[1]["content"].as_str().unwrap().contains("已截断"));
    }

    #[test]
    fn file_read_batch_skips_when_remaining_budget_too_small_for_next_file() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        fs::write(root.join("a.txt"), "a".repeat(800)).unwrap();
        fs::write(root.join("b.txt"), "b".repeat(800)).unwrap();

        let args = json!({
            "paths": ["a.txt", "b.txt"],
            "maxTotalBytes": 1000,
            "maxBytes": 10_000,
        });
        let out = execute_file_read(&args, root).expect("batch read");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["batchCapped"], true);
        assert_eq!(v["contentBytes"], 800);
        let files = v["files"].as_array().unwrap();
        assert_eq!(files[0]["content"].as_str().unwrap().len(), 800);
        assert!(files[1]["error"].as_str().unwrap().contains("剩余空间"));
    }

    #[test]
    fn file_read_paths_must_be_array_of_strings() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        let args = json!({ "paths": "not-an-array" });
        let err = execute_file_read(&args, root).unwrap_err();
        assert!(err.to_string().contains("paths"));
    }

    #[test]
    fn file_read_empty_paths_array_requires_path() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        let args = json!({ "paths": [] });
        let err = execute_file_read(&args, root).unwrap_err();
        assert!(err.to_string().contains("path"));
    }

    #[test]
    fn resolve_rejects_parent_escape() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        let err = resolve_within_workspace_root(root, "../outside").unwrap_err();
        assert!(err.to_string().contains("工作区") || err.to_string().contains("越出"));
    }

    #[test]
    fn accessible_path_absolute_outside_workspace() {
        let ws = tempfile::tempdir().expect("tmp");
        let other = tempfile::tempdir().expect("tmp");
        let f = other.path().join("out.txt");
        fs::write(&f, "x").unwrap();
        let abs = f.canonicalize().unwrap();
        let got = resolve_accessible_path(ws.path(), abs.to_str().unwrap()).unwrap();
        assert_eq!(got, abs);
    }

    #[test]
    fn file_list_non_recursive() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(root.join("sub").join("a.txt"), "1").unwrap();
        fs::write(root.join("b.txt"), "2").unwrap();
        let args = json!({"method": "list", "path": ".", "recursive": false, "entryType": "all"});
        let out = execute_file_tool(&args, root).expect("list");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let paths: Vec<&str> = v["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["path"].as_str().unwrap())
            .collect();
        assert!(paths.contains(&"b.txt"));
        assert!(paths.iter().any(|p| *p == "sub"));
    }

    #[test]
    fn resolve_allows_nested_file() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        let sub = root.join("a").join("b");
        fs::create_dir_all(&sub).unwrap();
        let f = sub.join("x.txt");
        let mut file = fs::File::create(&f).unwrap();
        writeln!(file, "hi").unwrap();

        let got = resolve_within_workspace_root(root, "a/b/x.txt").unwrap();
        assert_eq!(got, f.canonicalize().unwrap());
    }

    #[test]
    fn text_replace_lf_snippet_matches_crlf_file() {
        let text = "line1\r\nline2\r\n";
        let out = try_unique_text_replace(text, "line1\nline2", "A\nB").unwrap();
        assert_eq!(out, "A\r\nB\r\n");
    }

    #[test]
    fn text_replace_exact_lf_file() {
        let text = "line1\nline2\n";
        let out = try_unique_text_replace(text, "line1\nline2", "X\nY").unwrap();
        assert_eq!(out, "X\nY\n");
    }

    #[test]
    fn text_replace_crlf_snippet_matches_lf_file() {
        let text = "line1\nline2\n";
        let out = try_unique_text_replace(text, "line1\r\nline2", "P\r\nQ").unwrap();
        assert_eq!(out, "P\nQ\n");
    }

    #[test]
    fn text_replace_lf_snippet_matches_cr_only_file() {
        let text = "line1\rline2\r";
        let out = try_unique_text_replace(text, "line1\nline2", "A\nB").unwrap();
        assert_eq!(out, "A\nB\n");
    }
}
