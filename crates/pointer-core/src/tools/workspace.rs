//! Workspace-scoped file tools (cc-haha style). Root from settings `workspaceRoot`, else `current_dir`.
use super::{ToolHandler, ToolPrompt, ToolRegistry};
use crate::models::ToolDef;
use crate::storage;
use anyhow::{anyhow, Result};
use globset::{Glob, GlobSetBuilder};
use regex::RegexBuilder;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use walkdir::WalkDir;

const WORKSPACE_PROMPT: &str = include_str!("prompts/workspace.md");

const MAX_FILE_READ_BYTES: usize = 512 * 1024;
const MAX_GREP_RESULTS: usize = 200;
const MAX_GREP_FILE_BYTES: usize = 2 * 1024 * 1024;
const MAX_GLOB_RESULTS: usize = 500;
const MAX_WALK_DEPTH: usize = 64;
const CONTEXT_LINES: usize = 2;

/// Binary-ish extensions to skip in grep
const SKIP_EXT: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "ico", "pdf", "zip", "gz", "7z", "rar", "exe", "dll",
    "so", "dylib", "wasm", "mp3", "mp4", "avi", "mkv", "ttf", "woff", "woff2", "eot",
];

pub fn register_all(reg: &ToolRegistry) {
    register_file_read(reg);
    register_file_write(reg);
    register_file_edit(reg);
    register_glob_files(reg);
    register_grep_files(reg);
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

fn register_file_read(reg: &ToolRegistry) {
    let h: ToolHandler = Arc::new(|args| {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("缺少 path"))?;
        let line_start = args.get("lineStart").and_then(|v| v.as_u64()).unwrap_or(1).max(1) as usize;
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

        let root = resolve_tool_workspace_root()?;
        let full = resolve_within_workspace_root(&root, path)?;
        if !full.is_file() {
            return Err(anyhow!("不是文件: {}", full.display()));
        }
        let meta = fs::metadata(&full).map_err(|e| anyhow!("读取元数据失败: {e}"))?;
        if meta.len() > max_bytes as u64 {
            return Err(anyhow!(
                "文件过大 ({} bytes)，超过上限 {}。请缩小范围或使用 grep。",
                meta.len(),
                max_bytes
            ));
        }
        let bytes = fs::read(&full).map_err(|e| anyhow!("读取文件失败: {e}"))?;
        let text = String::from_utf8(bytes).map_err(|_| anyhow!("非 UTF-8 文本，无法作为文本读取"))?;
        let lines: Vec<&str> = text.lines().collect();
        let start_idx = line_start.saturating_sub(1).min(lines.len());
        let end_idx = if let Some(le) = line_end_exclusive {
            le.saturating_sub(1).min(lines.len()).max(start_idx)
        } else {
            lines.len()
        };
        let slice = &lines[start_idx..end_idx];
        let content = slice.join("\n");
        Ok(serde_json::json!({
            "path": full.display().to_string(),
            "lineStart": line_start,
            "lineEndExclusive": line_end_exclusive.unwrap_or(lines.len() + 1),
            "totalLines": lines.len(),
            "content": content,
            "truncated": false,
        })
        .to_string())
    });
    reg.register_with_prompt(
        ToolDef {
            name: "file_read".into(),
            description: "读取工作区内 UTF-8 文本文件。lineStart 为 1-based 起始行；lineEnd 为 1-based 结束行（不含），缺省读到末尾。"
                .into(),
            parameters_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "相对工作区根的路径，或工作区下的绝对路径" },
                    "lineStart": { "type": "integer", "description": "起始行，默认 1" },
                    "lineEnd": { "type": "integer", "description": "1-based 结束行（不含）；缺省读到文件末尾" },
                    "maxBytes": { "type": "integer", "description": "最大读取字节，默认 524288" }
                },
                "required": ["path"]
            }),
            risk_level: "low".into(),
            requires_approval: false,
        },
        Some(ToolPrompt {
            system_prompt: WORKSPACE_PROMPT.into(),
        }),
        h,
    );
}

fn register_file_write(reg: &ToolRegistry) {
    let h: ToolHandler = Arc::new(|args| {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("缺少 path"))?;
        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("缺少 content"))?;

        let root = resolve_tool_workspace_root()?;
        let full = resolve_within_workspace_root(&root, path)?;
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
    });
    reg.register_with_prompt(
        ToolDef {
            name: "file_write".into(),
            description: "创建或覆盖工作区内文本文件（整文件写入）。".into(),
            parameters_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["path", "content"]
            }),
            risk_level: "high".into(),
            requires_approval: true,
        },
        Some(ToolPrompt {
            system_prompt: WORKSPACE_PROMPT.into(),
        }),
        h,
    );
}

fn register_file_edit(reg: &ToolRegistry) {
    let h: ToolHandler = Arc::new(|args| {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("缺少 path"))?;
        let old_s = args
            .get("oldString")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("缺少 oldString"))?;
        let new_s = args
            .get("newString")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("缺少 newString"))?;
        if old_s.is_empty() {
            return Err(anyhow!("oldString 不能为空"));
        }

        let root = resolve_tool_workspace_root()?;
        let full = resolve_within_workspace_root(&root, path)?;
        if !full.is_file() {
            return Err(anyhow!("不是文件: {}", full.display()));
        }
        let text = fs::read_to_string(&full).map_err(|e| anyhow!("读取失败: {e}"))?;
        let count = text.matches(old_s).count();
        if count == 0 {
            return Err(anyhow!("未找到匹配的 oldString"));
        }
        if count > 1 {
            return Err(anyhow!("oldString 匹配到 {count} 处，必须唯一"));
        }
        let updated = text.replacen(old_s, new_s, 1);
        fs::write(&full, updated.as_bytes()).map_err(|e| anyhow!("写入失败: {e}"))?;
        Ok(serde_json::json!({
            "path": full.display().to_string(),
            "replaced": 1,
            "success": true
        })
        .to_string())
    });
    reg.register_with_prompt(
        ToolDef {
            name: "file_edit".into(),
            description: "在工作区内文本文件中用唯一匹配的 oldString 替换为 newString（单次替换）。".into(),
            parameters_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "oldString": { "type": "string" },
                    "newString": { "type": "string" }
                },
                "required": ["path", "oldString", "newString"]
            }),
            risk_level: "high".into(),
            requires_approval: true,
        },
        Some(ToolPrompt {
            system_prompt: WORKSPACE_PROMPT.into(),
        }),
        h,
    );
}

fn register_glob_files(reg: &ToolRegistry) {
    let h: ToolHandler = Arc::new(|args| {
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

        let root = resolve_tool_workspace_root()?;
        let glob = Glob::new(pattern).map_err(|e| anyhow!("glob 模式无效: {e}"))?;
        let mut builder = GlobSetBuilder::new();
        builder.add(glob);
        let set = builder.build().map_err(|e| anyhow!("glob 构建失败: {e}"))?;

        let mut matches = Vec::new();
        for entry in WalkDir::new(&root).max_depth(max_depth).into_iter().filter_map(|e| e.ok()) {
            if matches.len() >= max_results {
                break;
            }
            let p = entry.path();
            if p.is_file() {
                let rel = p.strip_prefix(&root).unwrap_or(p);
                let rel_norm = rel.to_string_lossy().replace('\\', "/");
                if set.is_match(Path::new(&rel_norm)) {
                    matches.push(rel_norm);
                }
            }
        }
        Ok(serde_json::json!({
            "root": root.display().to_string(),
            "pattern": pattern,
            "matches": matches,
            "count": matches.len(),
            "truncated": matches.len() >= max_results
        })
        .to_string())
    });
    reg.register_with_prompt(
        ToolDef {
            name: "glob_files".into(),
            description: "在工作区根下按 glob 模式（如 **/*.rs）列出文件路径，相对根目录。".into(),
            parameters_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "glob 模式，相对工作区根" },
                    "maxResults": { "type": "integer", "description": "最大条数，默认 500" },
                    "maxDepth": { "type": "integer", "description": "最大目录深度，默认 64" }
                },
                "required": ["pattern"]
            }),
            risk_level: "low".into(),
            requires_approval: false,
        },
        Some(ToolPrompt {
            system_prompt: WORKSPACE_PROMPT.into(),
        }),
        h,
    );
}

fn should_skip_grep(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.rsplit_once('.'))
        .map(|(_, ext)| SKIP_EXT.iter().any(|s| s.eq_ignore_ascii_case(ext)))
        .unwrap_or(false)
}

fn register_grep_files(reg: &ToolRegistry) {
    let h: ToolHandler = Arc::new(|args| {
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

        let root = resolve_tool_workspace_root()?;
        let search_root = if subdir.trim().is_empty() {
            root.clone()
        } else {
            resolve_within_workspace_root(&root, subdir)?
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
            let rel = p.strip_prefix(&root).unwrap_or(p);
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
            "root": root.display().to_string(),
            "pattern": pattern,
            "results": results,
            "count": results.len(),
            "truncated": results.len() >= max_results
        })
        .to_string())
    });
    reg.register_with_prompt(
        ToolDef {
            name: "grep_files".into(),
            description: "在工作区下用正则搜索文件内容，返回匹配行与少量上下文。跳过常见二进制扩展。".into(),
            parameters_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Rust 正则" },
                    "subdir": { "type": "string", "description": "可选，相对工作区根的子目录" },
                    "maxResults": { "type": "integer" },
                    "maxDepth": { "type": "integer" },
                    "contextLines": { "type": "integer", "description": "上下文行数，默认 2，最大 5" }
                },
                "required": ["pattern"]
            }),
            risk_level: "low".into(),
            requires_approval: false,
        },
        Some(ToolPrompt {
            system_prompt: WORKSPACE_PROMPT.into(),
        }),
        h,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn resolve_rejects_parent_escape() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        let err = resolve_within_workspace_root(root, "../outside").unwrap_err();
        assert!(err.to_string().contains("工作区") || err.to_string().contains("越出"));
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
}
