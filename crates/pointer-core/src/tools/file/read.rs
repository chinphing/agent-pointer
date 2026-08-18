use super::path::{path_display_abs, path_display_for_read_request, resolve_accessible_path};
use super::{json_u64_opt, FileToolLimits};
use crate::text_util::truncate_bytes;
use anyhow::{anyhow, Result};
use log::{info, warn};
use std::fs;
use std::io::{self, BufRead, BufReader};
use std::path::Path;

/// Read one line, never buffering more than `max_line_bytes` of that line.
/// Remaining bytes until `\n` (or EOF) are discarded. Returns `None` at EOF
/// with no leftover. The `bool` is true when the physical line was longer
/// than the cap.
fn read_line_capped<R: BufRead>(
    reader: &mut R,
    max_line_bytes: usize,
) -> io::Result<Option<(String, bool)>> {
    let mut collected: Vec<u8> = Vec::new();
    let mut skipped_rest = false;
    loop {
        let buf = reader.fill_buf()?;
        if buf.is_empty() {
            if collected.is_empty() && !skipped_rest {
                return Ok(None);
            }
            break;
        }
        if let Some(nl) = buf.iter().position(|&b| b == b'\n') {
            if !skipped_rest {
                let room = max_line_bytes.saturating_sub(collected.len());
                let take = nl.min(room);
                if take > 0 {
                    collected.extend_from_slice(&buf[..take]);
                }
                if nl > take {
                    skipped_rest = true;
                }
            }
            reader.consume(nl + 1);
            break;
        }
        if skipped_rest || collected.len() >= max_line_bytes {
            skipped_rest = true;
            let n = buf.len();
            reader.consume(n);
            continue;
        }
        let room = max_line_bytes - collected.len();
        let buf_len = buf.len();
        let n = buf_len.min(room);
        collected.extend_from_slice(&buf[..n]);
        reader.consume(n);
        if buf_len > n {
            skipped_rest = true;
        }
    }
    if collected.last() == Some(&b'\r') {
        collected.pop();
    }
    let text = String::from_utf8_lossy(&collected).into_owned();
    Ok(Some((text, skipped_rest)))
}

fn file_read_one_json(
    root: &Path,
    path_str: &str,
    line_start: usize,
    line_end_exclusive: Option<usize>,
    max_bytes: usize,
    line_max_bytes: usize,
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
    let mut reader = BufReader::new(file);
    let mut selected: Vec<String> = Vec::new();
    let mut selected_bytes: usize = 0;
    let mut truncated = false;
    let mut total_lines: usize = 0;
    let start_idx = line_start.saturating_sub(1);
    let end_idx = line_end_exclusive.map(|le| le.saturating_sub(1));

    loop {
        let (mut line, line_was_capped) = match read_line_capped(&mut reader, line_max_bytes) {
            Ok(Some(v)) => v,
            Ok(None) => break,
            Err(e) => {
                return serde_json::json!({
                    "path": full_display,
                    "error": format!("读取文件失败: {e}"),
                });
            }
        };
        if line_was_capped {
            truncated = true;
            line = truncate_bytes(&line, line_max_bytes);
        }
        let line_no = total_lines;
        total_lines += 1;

        let in_window = line_no >= start_idx && end_idx.map(|e| line_no < e).unwrap_or(true);
        if !in_window {
            continue;
        }
        if truncated && selected_bytes >= max_bytes {
            continue;
        }
        let add = if selected.is_empty() {
            line.len()
        } else {
            line.len() + 1
        };
        if selected_bytes + add > max_bytes {
            truncated = true;
            continue;
        }
        selected_bytes += add;
        selected.push(line);
        if line_was_capped {
            truncated = true;
        }
    }

    let content = selected.join("\n");
    let line_end_exclusive_out = line_end_exclusive.unwrap_or(total_lines.saturating_add(1));
    if truncated {
        info!(
            "file_read truncated path={} total_lines={} returned_bytes={} max_bytes={}",
            full_display, total_lines, selected_bytes, max_bytes
        );
    }
    let mut obj = serde_json::json!({
        "path": full_display,
        "lineStart": line_start,
        "lineEndExclusive": line_end_exclusive_out,
        "totalLines": total_lines,
        "content": content,
        "truncated": truncated,
    });
    if truncated {
        obj["warning"] = serde_json::json!(
            "output hit a hard size cap; use lineStart/lineEnd or grep — maxBytes cannot be raised above the runtime ceiling"
        );
    }
    obj
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

/// Core logic for `file_read` (one file per call). Tests use default caps.
pub(crate) fn execute_file_read(args: &serde_json::Value, root: &Path) -> Result<String> {
    execute_file_read_with(args, root, &FileToolLimits::default())
}

pub(crate) fn execute_file_read_with(
    args: &serde_json::Value,
    root: &Path,
    limits: &FileToolLimits,
) -> Result<String> {
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
    let ceiling = limits.read_max_bytes as u64;
    let requested_max = args
        .get("maxBytes")
        .or_else(|| args.get("max_bytes"))
        .and_then(|v| v.as_u64());
    if requested_max.is_some_and(|n| n > ceiling) {
        warn!(
            "file_read: maxBytes={} exceeds ceiling {}, clamping",
            requested_max.unwrap(),
            ceiling
        );
    }
    let max_bytes = requested_max.unwrap_or(ceiling).min(ceiling) as usize;

    let v = file_read_one_json(
        root,
        &path,
        line_start,
        line_end_exclusive,
        max_bytes,
        limits.line_max_bytes,
    );
    if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
        return Err(anyhow!("{err}"));
    }
    Ok(v.to_string())
}
