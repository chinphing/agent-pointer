use super::path::{path_display_abs, resolve_writable_path};
use crate::text_diff::compute_diff_lines;
use anyhow::{anyhow, Result};
use log::info;
use std::fs;
use std::path::Path;

fn resolve_file_write_content(value: Option<&serde_json::Value>) -> Result<String> {
    let v = value.ok_or_else(|| anyhow!("缺少 content"))?;
    match v {
        serde_json::Value::Null => Err(anyhow!("缺少 content")),
        serde_json::Value::String(s) => Ok(s.clone()),
        serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
            let mut s = serde_json::to_string_pretty(v)
                .map_err(|e| anyhow!("content JSON 序列化失败: {e}"))?;
            s.push('\n');
            Ok(s)
        }
        serde_json::Value::Number(n) => Ok(n.to_string()),
        serde_json::Value::Bool(b) => Ok(b.to_string()),
    }
}

pub(crate) fn execute_file_write_payload(args: &serde_json::Value, root: &Path) -> Result<String> {
    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("缺少 path"))?;
    let content = resolve_file_write_content(args.get("content"))?;

    let full = resolve_writable_path(root, path)?;
    let old_content = if full.exists() {
        fs::read_to_string(&full)
            .map_err(|e| anyhow!("读取待覆盖文件失败 {}: {e}", full.display()))?
    } else {
        info!(
            "file_write: creating new file path={}",
            path_display_abs(&full)
        );
        String::new()
    };
    if let Err(error) = crate::turn_file_baseline::ensure_baseline(&full, &old_content) {
        log::warn!(
            "file_write: turn baseline save failed path={}: {error:#}",
            path_display_abs(&full)
        );
    }
    if let Some(parent) = full.parent() {
        fs::create_dir_all(parent).map_err(|e| anyhow!("创建目录失败: {e}"))?;
    }
    fs::write(&full, content.as_bytes()).map_err(|e| anyhow!("写入失败: {e}"))?;
    let (_diff_lines, stats) = compute_diff_lines(&old_content, &content);
    info!(
        "file_write: path={}, bytes={}, created={}",
        path_display_abs(&full),
        content.len(),
        old_content.is_empty()
    );
    Ok(serde_json::json!({
        "path": path_display_abs(&full),
        "bytesWritten": content.as_bytes().len(),
        "success": true,
        "created": old_content.is_empty(),
        "stats": stats,
    })
    .to_string())
}
