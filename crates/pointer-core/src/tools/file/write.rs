use super::path::{path_display_abs, resolve_writable_path};
use anyhow::{anyhow, Result};
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
    // Read old content if file already existed
    let old_content = fs::read_to_string(&full).unwrap_or_default();
    if let Some(parent) = full.parent() {
        fs::create_dir_all(parent).map_err(|e| anyhow!("创建目录失败: {e}"))?;
    }
    fs::write(&full, content.as_bytes()).map_err(|e| anyhow!("写入失败: {e}"))?;
    Ok(serde_json::json!({
        "path": path_display_abs(&full),
        "bytesWritten": content.as_bytes().len(),
        "success": true,
        "old_content": old_content,
        "new_content": content
    })
    .to_string())
}
