use super::{
    json_str, json_u64_opt, MAX_FILE_READ_BATCH, MAX_FILE_READ_BATCH_TOTAL_BYTES_CLAMP,
    MAX_FILE_READ_BATCH_TOTAL_BYTES_DEFAULT, MAX_FILE_READ_BYTES, MIN_BATCH_TRUNCATE_REMAINING,
};
use super::path::{
    path_display_abs, path_display_for_read_request, resolve_accessible_path,
};
use anyhow::{anyhow, Result};
use std::fs;
use log::warn;
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

/// Core logic for `file:read` (batch `paths` only). Used by tests with an explicit root.
pub(crate) fn execute_file_read(args: &serde_json::Value, root: &Path) -> Result<String> {
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

    let paths_val = args.get("paths").ok_or_else(|| {
        anyhow!("缺少 paths；单文件读取请传 paths: [{{ \"path\": \"...\" }}]")
    })?;
    let arr = paths_val
        .as_array()
        .ok_or_else(|| anyhow!("paths 须为 JSON 数组"))?;
    if arr.is_empty() {
        return Err(anyhow!(
            "paths 不能为空；单文件读取请传 paths: [{{ \"path\": \"...\" }}]"
        ));
    }

    let specs = parse_file_read_batch_paths(arr, line_start, line_end_exclusive, max_bytes)?;
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
                "path": path_display_for_read_request(root, &spec.path),
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
                "path": path_display_for_read_request(root, &spec.path),
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

    Ok(serde_json::json!({
        "files": files,
        "maxTotalBytes": max_total_bytes,
        "contentBytes": content_bytes,
        "batchCapped": batch_capped,
    })
    .to_string())
}

/// One entry in a `file:read` batch (`paths` array).
#[derive(Debug, Clone)]
struct BatchReadSpec {
    path: String,
    line_start: usize,
    line_end_exclusive: Option<usize>,
    max_bytes: usize,
}

/// Each `paths` element must be an object `{ path, lineStart?, lineEnd?, maxBytes? }` (see
/// `tools/prompts/file.md`). Root-level `lineStart` / `lineEnd` / `maxBytes` apply when omitted on
/// the object.
fn parse_file_read_batch_paths(
    arr: &[serde_json::Value],
    default_line_start: usize,
    default_line_end_exclusive: Option<usize>,
    default_max_bytes: usize,
) -> Result<Vec<BatchReadSpec>> {
    let mut out = Vec::new();
    for elem in arr {
        match elem {
            serde_json::Value::String(_) => {
                return Err(anyhow!(
                    "paths 中每项须为 JSON 对象（含 path，可选 lineStart、lineEnd、maxBytes）；勿使用字符串元素"
                ));
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
                    "paths 中每项须为 JSON 对象（含 path，可选 lineStart、lineEnd、maxBytes）"
                ));
            }
        }
    }
    Ok(out)
}

