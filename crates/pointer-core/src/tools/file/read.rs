use super::path::{path_display_abs, path_display_for_read_request, resolve_accessible_path};
use super::{json_u64_opt_keys, FileToolLimits};
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
    offset: usize,
    limit: usize,
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
    let start_idx = offset.saturating_sub(1);
    let end_idx = start_idx.saturating_add(limit.max(1));

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
            // 超长行先截断；truncated 只在「窗口内」行被截断时置位，
            // 窗口外的超长行不影响返回内容，不应污染 truncated 标志。
            line = truncate_bytes(&line, line_max_bytes);
        }
        let line_no = total_lines;
        total_lines += 1;

        if line_no < start_idx {
            continue;
        }
        // Past the window: stop scanning so huge files are not fully read.
        if line_no >= end_idx {
            truncated = true;
            break;
        }
        if line_was_capped {
            truncated = true;
        }
        if truncated && selected_bytes >= max_bytes {
            break;
        }
        let add = if selected.is_empty() {
            line.len()
        } else {
            line.len() + 1
        };
        if selected_bytes + add > max_bytes {
            truncated = true;
            break;
        }
        selected_bytes += add;
        selected.push(line);
        if line_was_capped {
            truncated = true;
        }
    }

    let content = selected.join("\n");
    if truncated {
        info!(
            "file_read truncated path={} total_lines={} returned_bytes={} max_bytes={}",
            full_display, total_lines, selected_bytes, max_bytes
        );
    }
    let mut obj = serde_json::json!({
        "path": full_display,
        "offset": offset,
        "limit": limit,
        "totalLines": total_lines,
        "content": content,
        "truncated": truncated,
    });
    if truncated {
        obj["warning"] = serde_json::json!(
            "output hit a line or size cap; continue with a higher offset or grep — limit/maxBytes cannot be raised above the runtime ceiling"
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
#[allow(dead_code)] // 仅 #[cfg(test)] 引用（cargo check 不编译 tests）
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
    let offset = json_u64_opt_keys(args, &["offset"])
        .map(|(n, _)| n.max(1) as usize)
        .unwrap_or(1);
    let line_ceiling = limits.read_max_lines.max(1);
    let default_limit = (crate::models::DEFAULT_FILE_READ_LIMIT as usize).min(line_ceiling);
    let limit = match json_u64_opt_keys(args, &["limit"]) {
        Some((n, _)) => {
            let n = n.max(1) as usize;
            if n > line_ceiling {
                warn!("file_read: limit={n} exceeds ceiling {line_ceiling}, clamping path={path}");
                line_ceiling
            } else {
                n
            }
        }
        None => default_limit,
    };
    info!("file_read: offset={offset} limit={limit} path={path}");
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

    let v = file_read_one_json(root, &path, offset, limit, max_bytes, limits.line_max_bytes);
    if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
        return Err(anyhow!("{err}"));
    }
    Ok(v.to_string())
}
