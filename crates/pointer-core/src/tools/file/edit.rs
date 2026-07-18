use super::json_str;
use super::path::{path_display_abs, resolve_writable_path};
use anyhow::{anyhow, Result};
use log::info;
use similar::{ChangeTag, TextDiff};
use std::fs;
use std::path::{Path, PathBuf};

/// Collapse threshold: consecutive unchanged lines > this → folded.
const COLLAPSE_THRESHOLD: usize = 6;

/// Build diff lines from old/new content, folding long unchanged runs.
pub(super) fn compute_diff_lines(old: &str, new: &str) -> (Vec<serde_json::Value>, serde_json::Value) {
    let diff = TextDiff::from_lines(old, new);
    let mut all: Vec<serde_json::Value> = Vec::new();
    let mut adds = 0usize;
    let mut dels = 0usize;

    for c in diff.iter_all_changes() {
        let text = c.value().to_string().trim_end_matches('\n').to_string();
        match c.tag() {
            ChangeTag::Equal => {
                all.push(serde_json::json!({"type": "unchanged", "text": text}));
            }
            ChangeTag::Insert => {
                adds += 1;
                all.push(serde_json::json!({"type": "ins", "text": text}));
            }
            ChangeTag::Delete => {
                dels += 1;
                all.push(serde_json::json!({"type": "del", "text": text}));
            }
        }
    }

    // Post-process: fold consecutive unchanged runs > COLLAPSE_THRESHOLD
    let mut folded: Vec<serde_json::Value> = Vec::new();
    let mut i = 0;
    while i < all.len() {
        if all[i]["type"] == "unchanged" {
            let start = i;
            while i < all.len() && all[i]["type"] == "unchanged" {
                i += 1;
            }
            let count = i - start;
            if count > COLLAPSE_THRESHOLD {
                // Keep first 3 unchanged as context
                for j in start..start + 3 {
                    folded.push(all[j].clone());
                }
                // Collect the middle hidden lines
                let mut hidden: Vec<String> = Vec::new();
                for j in start + 3..i - 3 {
                    hidden.push(
                        all[j]["text"]
                            .as_str()
                            .unwrap_or("")
                            .to_string(),
                    );
                }
                folded.push(serde_json::json!({
                    "type": "collapse",
                    "text": hidden.len().to_string(),
                    "hidden": hidden,
                }));
                // Keep last 3 unchanged as context
                for j in i - 3..i {
                    folded.push(all[j].clone());
                }
            } else {
                for j in start..i {
                    folded.push(all[j].clone());
                }
            }
        } else {
            folded.push(all[i].clone());
            i += 1;
        }
    }

    let stats = serde_json::json!({ "adds": adds, "dels": dels });
    (folded, stats)
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
pub(crate) fn try_unique_text_replace(text: &str, old_s: &str, new_s: &str) -> Result<String> {
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

/// One `file_edit` replace. Returns (canonical_path, old_content, new_content).
fn file_edit_apply_one(root: &Path, path: &str, old_s: &str, new_s: &str) -> Result<(PathBuf, String, String)> {
    if old_s.is_empty() {
        return Err(anyhow!("oldString 不能为空"));
    }
    let full = resolve_writable_path(root, path)?;
    if !full.exists() {
        return Err(anyhow!("路径不存在: {}", full.display()));
    }
    let meta = fs::metadata(&full).map_err(|e| anyhow!("读取元数据失败: {e}"))?;
    if meta.is_dir() {
        return Err(anyhow!("目标是目录而非文件: {}", full.display()));
    }
    if !meta.is_file() {
        return Err(anyhow!("不是常规文件: {}", full.display()));
    }
    let text = fs::read_to_string(&full).map_err(|e| anyhow!("读取失败: {e}"))?;
    let updated = try_unique_text_replace(&text, old_s, new_s)?;
    fs::write(&full, updated.as_bytes()).map_err(|e| anyhow!("写入失败: {e}"))?;
    Ok((full, text, updated))
}

/// Resolve a single edit: `path` + `oldString` + `newString` (one file per call).
fn resolve_single_edit(
    args: &serde_json::Value,
) -> Result<(String, String, String)> {
    if args.get("edits").is_some() {
        return Err(anyhow!(
            "file_edit 为单文件工具：请传 path、oldString、newString。多文件请并发多次 file_edit，不要传 edits"
        ));
    }
    let path = args
        .get("path")
        .or_else(|| args.get("file"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("缺少 path"))?;
    let old_s = json_str(args, "oldString", "old_string")
        .ok_or_else(|| anyhow!("缺少 oldString（或 old_string）"))?
        .to_string();
    let new_s = json_str(args, "newString", "new_string")
        .ok_or_else(|| anyhow!("缺少 newString（或 new_string）"))?
        .to_string();
    Ok((path, old_s, new_s))
}

pub(crate) fn execute_file_edit_payload(args: &serde_json::Value, root: &Path) -> Result<String> {
    let (path, old_s, new_s) = resolve_single_edit(args)?;
    info!("file_edit: single replace under workspace path={path}");
    let (full, old_content, new_content) = file_edit_apply_one(root, &path, &old_s, &new_s)?;
    let (diff_lines, diff_stats) = compute_diff_lines(&old_content, &new_content);
    Ok(serde_json::json!({
        "path": path_display_abs(&full),
        "success": true,
        "replaced": 1,
        "diff_lines": diff_lines,
        "diff_stats": diff_stats,
    })
    .to_string())
}
