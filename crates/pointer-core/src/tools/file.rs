//! Workspace-scoped file tools: single registry tool `file` with `method` (like Computer `mouse:method`).
//! Root from settings `workspaceRoot`, else `current_dir`.
use super::{ToolEntry, ToolHandler, ToolRegistry};
use crate::storage;
use anyhow::{anyhow, Result};
use log::{info, warn};
use globset::{Glob, GlobSetBuilder};
use regex::{Regex, RegexBuilder};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use walkdir::WalkDir;

/// Doc for registry tool `file`; keep in sync with `prompts/file.md`.
const FILE_MD: &str = include_str!("prompts/file.md");

const MAX_FILE_READ_BYTES: usize = 256 * 1024;
/// Max files per `file` read batch (`paths`). **Keep in sync** with `prompts/file.md` Parameters section.
const MAX_FILE_READ_BATCH: usize = 32;
/// Max entries per `file:edit` batch (`edits`). **Keep in sync** with `prompts/file.md` Parameters section.
const MAX_FILE_EDIT_BATCH: usize = 32;
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
    let line_start = json_u64_opt(args, "lineStart", "line_start")
        .unwrap_or(1)
        .max(1) as usize;
    // lineEnd: 1-based exclusive (与 Rust range 一致：读到「该行之前」)。缺省读到文件末尾。
    let line_end_exclusive = json_u64_opt(args, "lineEnd", "line_end").map(|n| n.max(1) as usize);
    let max_bytes = args
        .get("maxBytes")
        .or_else(|| args.get("max_bytes"))
        .and_then(|v| v.as_u64())
        .unwrap_or(MAX_FILE_READ_BYTES as u64)
        .min(MAX_FILE_READ_BYTES as u64) as usize;

    let batch_specs: Option<Vec<BatchReadSpec>> = match args.get("paths") {
        None => None,
        Some(v) => {
            let arr = v
                .as_array()
                .ok_or_else(|| anyhow!("paths 须为字符串数组或对象数组"))?;
            Some(parse_file_read_batch_paths(
                arr,
                line_start,
                line_end_exclusive,
                max_bytes,
            )?)
        }
    };

    let use_batch = batch_specs
        .as_ref()
        .map(|p| !p.is_empty())
        .unwrap_or(false);

    if use_batch {
        let specs = batch_specs.unwrap_or_default();
        if specs.len() > MAX_FILE_READ_BATCH {
            return Err(anyhow!(
                "一次最多读取 {} 个文件（当前 {}）",
                MAX_FILE_READ_BATCH,
                specs.len()
            ));
        }
        let max_total_bytes = args
            .get("maxTotalBytes")
            .or_else(|| args.get("max_total_bytes"))
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
        let mut files: Vec<serde_json::Value> = Vec::with_capacity(specs.len());

        for spec in &specs {
            if budget_done || content_bytes >= max_total_bytes {
                files.push(serde_json::json!({
                    "path": spec.path,
                    "error": "未读取：本批正文已达 maxTotalBytes 上限。请减少 paths、为各 path 设置 lineStart/lineEnd、降低 maxBytes，或拆成多次 file:read。",
                }));
                batch_capped = true;
                continue;
            }

            let mut v = file_read_one_json(
                root,
                &spec.path,
                spec.line_start,
                spec.line_end_exclusive,
                spec.max_bytes,
            );

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
                    "path": spec.path,
                    "error": format!(
                        "本批剩余空间过小（{} 字节），无法容纳此文件正文。请提高 maxTotalBytes、减少 paths，或为各 path 设置 lineStart/lineEnd。",
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

fn json_u64_opt(args: &serde_json::Value, camel: &str, snake: &str) -> Option<u64> {
    args.get(camel)
        .and_then(|v| v.as_u64())
        .or_else(|| args.get(snake).and_then(|v| v.as_u64()))
}

/// One entry in a `file:read` batch (`paths` array).
#[derive(Debug, Clone)]
struct BatchReadSpec {
    path: String,
    line_start: usize,
    line_end_exclusive: Option<usize>,
    max_bytes: usize,
}

/// `paths` may be string paths (shared defaults) or objects `{ path, lineStart?, lineEnd?, maxBytes? }`.
fn parse_file_read_batch_paths(
    arr: &[serde_json::Value],
    default_line_start: usize,
    default_line_end_exclusive: Option<usize>,
    default_max_bytes: usize,
) -> Result<Vec<BatchReadSpec>> {
    let mut out = Vec::new();
    for elem in arr {
        match elem {
            serde_json::Value::String(s) => {
                let p = s.trim();
                if p.is_empty() {
                    continue;
                }
                out.push(BatchReadSpec {
                    path: p.to_string(),
                    line_start: default_line_start,
                    line_end_exclusive: default_line_end_exclusive,
                    max_bytes: default_max_bytes,
                });
            }
            serde_json::Value::Object(_) => {
                let path = json_str(elem, "path", "file")
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| {
                        anyhow!("paths 中每个对象须包含非空 path（可使用别名 file）")
                    })?;
                let line_start = match elem.get("lineStart").or_else(|| elem.get("line_start")) {
                    None => default_line_start,
                    Some(serde_json::Value::Null) => 1,
                    Some(v) => v
                        .as_u64()
                        .ok_or_else(|| anyhow!("paths 对象中的 lineStart 须为 JSON 无符号整数"))?
                        .max(1) as usize,
                };
                let line_end_exclusive = match elem.get("lineEnd").or_else(|| elem.get("line_end")) {
                    None => default_line_end_exclusive,
                    Some(serde_json::Value::Null) => None,
                    Some(v) => Some(
                        v.as_u64()
                            .ok_or_else(|| anyhow!("paths 对象中的 lineEnd 须为 JSON 无符号整数"))?
                            .max(1) as usize,
                    ),
                };
                let max_bytes = match elem.get("maxBytes").or_else(|| elem.get("max_bytes")) {
                    None => default_max_bytes,
                    Some(v) => v
                        .as_u64()
                        .ok_or_else(|| anyhow!("paths 对象中的 maxBytes 须为 JSON 无符号整数"))?
                        .min(MAX_FILE_READ_BYTES as u64) as usize,
                };
                out.push(BatchReadSpec {
                    path: path.to_string(),
                    line_start,
                    line_end_exclusive,
                    max_bytes,
                });
            }
            _ => {
                return Err(anyhow!(
                    "paths 须为字符串数组，或包含 path 的对象数组（可混用字符串与对象）"
                ));
            }
        }
    }
    Ok(out)
}

fn parse_file_edit_batch_entries(arr: &[serde_json::Value]) -> Result<Vec<(String, String, String)>> {
    let mut out = Vec::with_capacity(arr.len());
    for (i, elem) in arr.iter().enumerate() {
        if !elem.is_object() {
            return Err(anyhow!(
                "edits[{}] 须为 JSON 对象（含 path、oldString、newString）",
                i
            ));
        }
        let path = json_str(elem, "path", "file")
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow!("edits[{}] 缺少非空 path（可使用别名 file）", i))?;
        let old_s = json_str(elem, "oldString", "old_string")
            .ok_or_else(|| anyhow!("edits[{}] 缺少 oldString（或 old_string）", i))?;
        let new_s = json_str(elem, "newString", "new_string")
            .ok_or_else(|| anyhow!("edits[{}] 缺少 newString（或 new_string）", i))?;
        out.push((path.to_string(), old_s.to_string(), new_s.to_string()));
    }
    Ok(out)
}

/// One `file:edit` replace (workspace-relative `path` only). Returns resolved path on success.
fn file_edit_apply_one(root: &Path, path: &str, old_s: &str, new_s: &str) -> Result<PathBuf> {
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
    Ok(full)
}

fn execute_file_edit_payload(args: &serde_json::Value, root: &Path) -> Result<String> {
    if let Some(edits_val) = args.get("edits") {
        if !edits_val.is_array() {
            return Err(anyhow!("edits 须为对象数组，每项含 path、oldString、newString"));
        }
        let arr = edits_val.as_array().expect("is_array checked");
        if arr.is_empty() {
            return Err(anyhow!(
                "edits 至少包含一项；单文件编辑请使用 path、oldString、newString"
            ));
        }
        let has_flat = args
            .get("path")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .is_some_and(|s| !s.is_empty())
            || json_str(args, "oldString", "old_string").is_some()
            || json_str(args, "newString", "new_string").is_some();
        if has_flat {
            return Err(anyhow!(
                "批处理时不要同时传 edits 与顶层的 path、oldString、newString"
            ));
        }
        if arr.len() > MAX_FILE_EDIT_BATCH {
            return Err(anyhow!(
                "一次最多应用 {} 处编辑（当前 {}）",
                MAX_FILE_EDIT_BATCH,
                arr.len()
            ));
        }
        let entries = parse_file_edit_batch_entries(arr)?;
        info!(
            "file:edit batch: {} workspace-relative path(s)",
            entries.len()
        );
        let mut files: Vec<serde_json::Value> = Vec::with_capacity(entries.len());
        let mut failures: usize = 0;
        for (path, old_s, new_s) in entries {
            match file_edit_apply_one(root, &path, &old_s, &new_s) {
                Ok(full) => {
                    files.push(serde_json::json!({
                        "path": full.display().to_string(),
                        "success": true,
                        "replaced": 1,
                    }));
                }
                Err(e) => {
                    failures += 1;
                    warn!("file:edit batch entry failed for {}: {}", path, e);
                    files.push(serde_json::json!({
                        "path": path,
                        "success": false,
                        "error": e.to_string(),
                    }));
                }
            }
        }
        let batch_partial_failure = failures > 0;
        if batch_partial_failure {
            warn!(
                "file:edit batch completed with {} failure(s) out of {}",
                failures,
                files.len()
            );
        }
        return Ok(serde_json::json!({
            "files": files,
            "successCount": files.len() - failures,
            "failureCount": failures,
            "batchPartialFailure": batch_partial_failure,
        })
        .to_string());
    }

    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("缺少 path；多文件编辑请传 edits 数组"))?;
    let old_s = json_str(args, "oldString", "old_string")
        .ok_or_else(|| anyhow!("缺少 oldString（或 old_string）"))?;
    let new_s = json_str(args, "newString", "new_string")
        .ok_or_else(|| anyhow!("缺少 newString（或 new_string）"))?;

    let full = file_edit_apply_one(root, path, old_s, new_s)?;
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

/// Append grep hits for one UTF-8 file into `results`.
/// Each hit's `path` is relative to `rel_strip_base` (walk root or workspace root).
fn grep_one_file(
    rel_strip_base: &Path,
    file_path: &Path,
    re: &Regex,
    context: usize,
    max_results: usize,
    results: &mut Vec<serde_json::Value>,
) -> Result<()> {
    if results.len() >= max_results {
        return Ok(());
    }
    if should_skip_grep(file_path) {
        return Ok(());
    }
    let meta = match fs::metadata(file_path) {
        Ok(m) => m,
        Err(_) => return Ok(()),
    };
    if meta.len() > MAX_GREP_FILE_BYTES as u64 {
        return Ok(());
    }
    let bytes = match fs::read(file_path) {
        Ok(b) => b,
        Err(_) => return Ok(()),
    };
    let text = match String::from_utf8(bytes) {
        Ok(t) => t,
        Err(_) => return Ok(()),
    };
    let rel = file_path.strip_prefix(rel_strip_base).unwrap_or(file_path);
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
    Ok(())
}

fn execute_file_grep_payload(args: &serde_json::Value, root: &Path) -> Result<String> {
    let root = root
        .canonicalize()
        .map_err(|e| anyhow!("工作区根无效: {e}"))?;
    let pattern = args
        .get("pattern")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("缺少 pattern"))?;
    if pattern.len() > 512 {
        return Err(anyhow!("正则过长"));
    }
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

    if let Some(obj) = args.as_object() {
        if obj.contains_key("subdir") {
            return Err(anyhow!("grep 已移除参数 subdir，请使用 path（文件或目录）"));
        }
    }

    let path_arg = args.get("path").and_then(|v| v.as_str()).unwrap_or("").trim();

    enum GrepScope {
        Walk { start: PathBuf, max_depth: usize },
        SingleFile { file: PathBuf },
    }

    let scope = if path_arg.is_empty() {
        GrepScope::Walk {
            start: root.to_path_buf(),
            max_depth,
        }
    } else {
        let p = resolve_accessible_path(&root, path_arg)?;
        if p.is_dir() {
            GrepScope::Walk {
                start: p,
                max_depth,
            }
        } else if p.is_file() {
            info!("file:grep: single file {}", p.display());
            GrepScope::SingleFile { file: p }
        } else {
            return Err(anyhow!(
                "grep path 必须是已存在的文件或目录: {}",
                p.display()
            ));
        }
    };

    let mut results: Vec<serde_json::Value> = Vec::new();
    let (root_field, single_file) = match &scope {
        GrepScope::Walk { start, .. } => (start.display().to_string(), false),
        GrepScope::SingleFile { file } => {
            let rel = file.strip_prefix(&root).unwrap_or(file.as_path());
            (rel.to_string_lossy().replace('\\', "/"), true)
        }
    };

    match scope {
        GrepScope::SingleFile { file } => {
            grep_one_file(&root, &file, &re, context, max_results, &mut results)?;
        }
        GrepScope::Walk { start, max_depth } => {
            for entry in WalkDir::new(&start)
                .max_depth(max_depth)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                if results.len() >= max_results {
                    break;
                }
                let p = entry.path();
                if !p.is_file() {
                    continue;
                }
                grep_one_file(&start, p, &re, context, max_results, &mut results)?;
            }
        }
    }

    let mut out = serde_json::json!({
        "root": root_field,
        "pattern": pattern,
        "results": results,
        "count": results.len(),
        "truncated": results.len() >= max_results
    });
    if single_file {
        out["singleFile"] = serde_json::json!(true);
    }
    Ok(out.to_string())
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
    fn file_read_batch_per_path_line_ranges() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        fs::write(root.join("a.txt"), "l1\nl2\nl3\nl4\n").unwrap();
        fs::write(root.join("b.txt"), "a\nb\nc\nd\ne\n").unwrap();

        let args = json!({
            "paths": [
                { "path": "a.txt", "lineStart": 2, "lineEnd": 4 },
                { "path": "b.txt", "lineStart": 1, "lineEnd": 3 }
            ]
        });
        let out = execute_file_read(&args, root).expect("batch read");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let files = v["files"].as_array().unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0]["content"].as_str().unwrap(), "l2\nl3");
        assert_eq!(files[1]["content"].as_str().unwrap(), "a\nb");
    }

    #[test]
    fn file_read_batch_mixed_string_and_object_inherits_root_line_end() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        fs::write(root.join("x.txt"), "p1\np2\np3\np4\n").unwrap();
        fs::write(root.join("y.txt"), "q1\nq2\nq3\n").unwrap();

        let args = json!({
            "lineStart": 1,
            "lineEnd": 4,
            "paths": [
                "x.txt",
                { "path": "y.txt", "lineStart": 2 }
            ]
        });
        let out = execute_file_read(&args, root).expect("batch read");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let files = v["files"].as_array().unwrap();
        assert_eq!(files[0]["content"].as_str().unwrap(), "p1\np2\np3");
        assert_eq!(files[1]["content"].as_str().unwrap(), "q2\nq3");
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
    fn file_edit_single_path_still_ok() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        fs::write(root.join("z.txt"), "foo\n").unwrap();
        let args = json!({
            "path": "z.txt",
            "oldString": "foo",
            "newString": "bar"
        });
        let out = execute_file_edit_payload(&args, root).expect("edit");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["success"], true);
        assert_eq!(v["replaced"], 1);
        assert!(v["path"].as_str().unwrap().contains("z.txt"));
        assert_eq!(fs::read_to_string(root.join("z.txt")).unwrap().trim(), "bar");
    }

    #[test]
    fn file_edit_batch_two_files() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        fs::write(root.join("a.txt"), "A\n").unwrap();
        fs::write(root.join("b.txt"), "B\n").unwrap();
        let args = json!({
            "edits": [
                { "path": "a.txt", "oldString": "A", "newString": "AA" },
                { "path": "b.txt", "oldString": "B", "newString": "BB" }
            ]
        });
        let out = execute_file_edit_payload(&args, root).expect("batch edit");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["batchPartialFailure"], false);
        assert_eq!(v["successCount"], 2);
        assert_eq!(v["failureCount"], 0);
        let files = v["files"].as_array().unwrap();
        assert!(files[0]["success"].as_bool().unwrap());
        assert!(files[1]["success"].as_bool().unwrap());
        assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap().trim(), "AA");
        assert_eq!(fs::read_to_string(root.join("b.txt")).unwrap().trim(), "BB");
    }

    #[test]
    fn file_edit_batch_partial_failure() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        fs::write(root.join("ok.txt"), "x\n").unwrap();
        let args = json!({
            "edits": [
                { "path": "ok.txt", "oldString": "x", "newString": "y" },
                { "path": "missing.txt", "oldString": "a", "newString": "b" }
            ]
        });
        let out = execute_file_edit_payload(&args, root).expect("batch edit");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["batchPartialFailure"], true);
        assert_eq!(v["successCount"], 1);
        assert_eq!(v["failureCount"], 1);
        let files = v["files"].as_array().unwrap();
        assert!(files[0]["success"].as_bool().unwrap());
        assert_eq!(files[1]["success"], false);
        assert!(files[1]["error"].as_str().unwrap().len() > 0);
        assert_eq!(fs::read_to_string(root.join("ok.txt")).unwrap().trim(), "y");
    }

    #[test]
    fn file_edit_batch_rejects_edits_with_top_level_path() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        let args = json!({
            "path": "x.txt",
            "edits": [{ "path": "x.txt", "oldString": "a", "newString": "b" }]
        });
        let err = execute_file_edit_payload(&args, root).unwrap_err();
        assert!(err.to_string().contains("同时"));
    }

    #[test]
    fn file_grep_path_searches_only_that_file() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        fs::write(root.join("one.rs"), "fn alpha() {}\nfn beta() {}\n").unwrap();
        fs::write(root.join("two.rs"), "fn alpha_dup() {}\n").unwrap();
        let args = json!({
            "pattern": "alpha",
            "path": "one.rs",
            "maxResults": 20,
        });
        let out = execute_file_grep_payload(&args, root).expect("grep");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["singleFile"], true);
        let results = v["results"].as_array().unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["path"], "one.rs");
    }

    #[test]
    fn file_grep_path_nested_file_single_file_scope() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        fs::create_dir_all(root.join("pkg")).unwrap();
        fs::write(root.join("pkg").join("a.java"), "class A { void m() {} }\n").unwrap();
        fs::write(root.join("pkg").join("b.java"), "class A { void n() {} }\n").unwrap();
        let args = json!({
            "pattern": "class A",
            "path": "pkg/a.java",
            "maxResults": 20,
        });
        let out = execute_file_grep_payload(&args, root).expect("grep");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["singleFile"], true);
        let results = v["results"].as_array().unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["path"], "pkg/a.java");
    }

    #[test]
    fn file_grep_path_directory_scans_all_files_under() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        fs::create_dir_all(root.join("pkg")).unwrap();
        fs::write(root.join("pkg").join("a.java"), "class A {}\n").unwrap();
        fs::write(root.join("pkg").join("b.java"), "class B {}\n").unwrap();
        let args = json!({
            "pattern": "class",
            "path": "pkg",
            "maxResults": 20,
        });
        let out = execute_file_grep_payload(&args, root).expect("grep");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(v.get("singleFile").is_none() || v["singleFile"] == false);
        let results = v["results"].as_array().unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn file_grep_subdir_parameter_rejected() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        fs::write(root.join("x.txt"), "a\n").unwrap();
        let args = json!({
            "pattern": "a",
            "subdir": "x.txt",
        });
        let err = execute_file_grep_payload(&args, root).unwrap_err();
        assert!(err.to_string().contains("subdir"));
        let args_ok = json!({
            "pattern": "a",
            "path": "x.txt",
            "maxResults": 20,
        });
        execute_file_grep_payload(&args_ok, root).expect("grep with path");
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
