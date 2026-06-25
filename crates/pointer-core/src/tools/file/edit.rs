use super::{json_str, MAX_FILE_EDIT_BATCH};
use super::path::{path_display_abs, resolve_writable_path};
use anyhow::{anyhow, Result};
use log::{info, warn};
use std::fs;
use std::path::{Path, PathBuf};

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

/// One `file:edit` replace. `path` is workspace-relative or absolute under allowed write roots.
fn file_edit_apply_one(root: &Path, path: &str, old_s: &str, new_s: &str) -> Result<PathBuf> {
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
    Ok(full)
}

pub(crate) fn execute_file_edit_payload(args: &serde_json::Value, root: &Path) -> Result<String> {
    let has_flat = args
        .get("path")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .is_some_and(|s| !s.is_empty())
        || json_str(args, "oldString", "old_string").is_some()
        || json_str(args, "newString", "new_string").is_some();

    let edits_val = args.get("edits").ok_or_else(|| {
        anyhow!(
            "file:edit 仅支持 edits 数组（每项含 path、oldString、newString）；单文件请传仅含一项的 edits 数组"
        )
    })?;

    if !edits_val.is_array() {
        return Err(anyhow!("edits 须为对象数组，每项含 path、oldString、newString"));
    }
    let arr = edits_val.as_array().expect("is_array checked");

    if has_flat {
        return Err(anyhow!(
            "file:edit 不要同时使用顶层 path、oldString、newString 与 edits；请只使用 edits 数组"
        ));
    }

    if arr.is_empty() {
        return Err(anyhow!(
            "edits 至少包含一项；单文件编辑请传仅一项的 edits 数组"
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
        "file:edit: {} replacement(s) under workspace",
        entries.len()
    );
    let mut files: Vec<serde_json::Value> = Vec::with_capacity(entries.len());
    let mut failures: usize = 0;
    for (path, old_s, new_s) in entries {
        match file_edit_apply_one(root, &path, &old_s, &new_s) {
            Ok(full) => {
                files.push(serde_json::json!({
                    "path": path_display_abs(&full),
                    "success": true,
                    "replaced": 1,
                }));
            }
            Err(e) => {
                failures += 1;
                warn!("file:edit entry failed for {}: {}", path, e);
                let disp = resolve_writable_path(root, &path)
                    .map(|p| path_display_abs(&p))
                    .unwrap_or_else(|_| path.clone());
                files.push(serde_json::json!({
                    "path": disp,
                    "success": false,
                    "error": e.to_string(),
                }));
            }
        }
    }
    let batch_partial_failure = failures > 0;
    if batch_partial_failure {
        warn!(
            "file:edit completed with {} failure(s) out of {}",
            failures,
            files.len()
        );
    }
    Ok(serde_json::json!({
        "files": files,
        "successCount": files.len() - failures,
        "failureCount": failures,
        "batchPartialFailure": batch_partial_failure,
    })
    .to_string())
}
