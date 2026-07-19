use super::{json_u64_opt, MAX_FILE_READ_BYTES};
use super::path::{
    path_display_abs, path_display_for_read_request, resolve_accessible_path,
};
use anyhow::{anyhow, Result};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;

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
                "path": path_display_for_read_request(root, path_str),
                "error": e.to_string(),
            });
        }
    };
    let full_display = path_display_abs(&full);
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
    // Whole-file size gate only applies to unbounded reads (no line window).
    // With lineStart/lineEnd, maxBytes limits the returned window content instead.
    let has_line_window = line_start > 1 || line_end_exclusive.is_some();
    if !has_line_window && meta.len() > max_bytes as u64 {
        return serde_json::json!({
            "path": full_display,
            "error": format!(
                "文件过大 ({} bytes)，超过上限 {}。请缩小范围或使用 grep。",
                meta.len(),
                max_bytes
            ),
        });
    }

    let file = match fs::File::open(&full) {
        Ok(f) => f,
        Err(e) => {
            return serde_json::json!({
                "path": full_display,
                "error": format!("读取文件失败: {e}"),
            });
        }
    };
    let reader = BufReader::new(file);
    let mut selected: Vec<String> = Vec::new();
    let mut selected_bytes: usize = 0;
    let mut truncated = false;
    let mut total_lines: usize = 0;
    let start_idx = line_start.saturating_sub(1);
    let end_idx = line_end_exclusive.map(|le| le.saturating_sub(1));

    for line_result in reader.lines() {
        let line = match line_result {
            Ok(l) => l,
            Err(e) => {
                return serde_json::json!({
                    "path": full_display,
                    "error": format!("读取文件失败: {e}"),
                });
            }
        };
        // Reject non-UTF-8 via lossy check: BufRead::lines requires UTF-8 and errors on invalid.
        let line_no = total_lines; // 0-based before increment
        total_lines += 1;

        let in_window = line_no >= start_idx && end_idx.map(|e| line_no < e).unwrap_or(true);
        if !in_window {
            continue;
        }
        if truncated {
            continue;
        }
        let add = if selected.is_empty() {
            line.len()
        } else {
            line.len() + 1 // joining newline
        };
        if selected_bytes + add > max_bytes {
            truncated = true;
            continue;
        }
        selected_bytes += add;
        selected.push(line);
    }

    let content = selected.join("\n");
    let line_end_exclusive_out = line_end_exclusive.unwrap_or(total_lines.saturating_add(1));
    serde_json::json!({
        "path": full_display,
        "lineStart": line_start,
        "lineEndExclusive": line_end_exclusive_out,
        "totalLines": total_lines,
        "content": content,
        "truncated": truncated,
    })
}

fn resolve_file_read_path(args: &serde_json::Value) -> Result<String> {
    if args.get("paths").is_some() {
        return Err(anyhow!(
            "file_read 为单文件工具：请传 path（或 file）。多文件请并发多次 file_read，不要传 paths"
        ));
    }
    args.get("path")
        .or_else(|| args.get("file"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            anyhow!(
                "缺少 path；单文件请传 path: \"...\"（Windows 优先用正斜杠，例如 C:/project/foo.rs）"
            )
        })
}

/// Core logic for `file_read` (one file per call). Used by tests with an explicit root.
pub(crate) fn execute_file_read(args: &serde_json::Value, root: &Path) -> Result<String> {
    if args.is_null() {
        return Err(anyhow!(
            "tool arguments 无效或不是合法 JSON（常见原因：Windows 路径里的 \\ 未写成 \\\\）。\
请传 path: \"...\"；Windows 优先用正斜杠，例如 C:/project/foo.rs"
        ));
    }

    let path = resolve_file_read_path(args)?;
    let line_start = json_u64_opt(args, "lineStart", "line_start")
        .unwrap_or(1)
        .max(1) as usize;
    let line_end_exclusive = json_u64_opt(args, "lineEnd", "line_end").map(|n| n.max(1) as usize);
    let max_bytes = args
        .get("maxBytes")
        .or_else(|| args.get("max_bytes"))
        .and_then(|v| v.as_u64())
        .unwrap_or(MAX_FILE_READ_BYTES as u64)
        .min(MAX_FILE_READ_BYTES as u64) as usize;

    let v = file_read_one_json(root, &path, line_start, line_end_exclusive, max_bytes);
    if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
        return Err(anyhow!("{err}"));
    }
    Ok(v.to_string())
}
