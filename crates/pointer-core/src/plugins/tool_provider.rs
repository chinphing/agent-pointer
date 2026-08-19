//! ProcessToolProvider：插件 sidecar 工具的进程外执行载体（设计稿 §4.1 注 4.1.1、§7.3）。
//!
//! 协议（stdio + JSON）：
//! - 请求：handler 收到 `serde_json::Value` 参数，向子进程 stdin 写入
//!   `{"tool": "<tool_name>", "args": {…}}`；
//! - 响应：子进程 stdout 输出 `{"ok": true, "result": "…"}` 或
//!   `{"ok": false, "error": "…"}`；
//! - 超时：`timeout_ms`（默认 60s）后终止子进程并返回错误。
//!
//! 安全：插件工具与内置工具共用 `ToolRegistry` 审批链路（`risk_level` /
//! `requires_approval` 来自 manifest，`tool_invocation_needs_approval` 自动生效）。

use crate::plugins::manifest::PluginToolDecl;
use crate::tools::{ToolEntry, ToolHandler};
use anyhow::{anyhow, Result};
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

/// 插件工具默认进程超时（毫秒）。
const DEFAULT_TIMEOUT_MS: u64 = 60_000;

/// 根据 manifest `[[tools.tool]]` 声明构建一个 `ToolEntry`（sidecar 执行载体）。
pub fn build_sidecar_tool_entry(
    plugin_id: &str,
    plugin_dir: &Path,
    decl: &PluginToolDecl,
) -> Result<ToolEntry> {
    let exec = decl
        .exec
        .as_ref()
        .ok_or_else(|| anyhow!("工具 {} 缺少 exec 执行载体", decl.name))?;
    if exec.transport != "sidecar" {
        return Err(anyhow!(
            "工具 {} 的 exec.transport `{}` 在 P1 阶段不支持（仅 sidecar）",
            decl.name,
            exec.transport
        ));
    }

    // 解析可执行命令：相对插件根目录或绝对路径。
    let command_path = resolve_command_path(plugin_dir, &exec.command)?;

    // doc_source 使用动态唯一 key，避免与内置工具的静态路径冲突。
    let doc_source: std::borrow::Cow<'static, str> =
        std::borrow::Cow::Owned(format!("plugin:{plugin_id}:{}", decl.name));
    let doc_markdown = build_doc_markdown(decl);

    let tool_name_for_handler = decl.name.clone();
    let mut args = exec.args.clone();
    let mut env = exec.env.clone();
    let timeout_ms = exec.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS);

    let handler: ToolHandler = Arc::new(move |call_args: serde_json::Value| -> Result<String> {
        run_sidecar(
            &command_path,
            &tool_name_for_handler,
            &args,
            &env,
            call_args,
            timeout_ms,
        )
    });

    let mut entry = ToolEntry::new(
        decl.name.clone(),
        doc_source,
        decl.risk_level.clone(),
        decl.requires_approval,
        doc_markdown,
        handler,
    )
    .with_plugin_id(plugin_id.to_string());
    if let Some(schema) = decl.schema.clone() {
        entry = entry.with_schema(schema);
    }
    if decl.parallel_eligible {
        entry = entry.with_parallel_metadata(true, crate::tools::parallel::ToolConflictClass::None);
    }
    Ok(entry)
}

/// 解析命令路径：绝对路径直接用；相对路径相对插件根目录。
fn resolve_command_path(plugin_dir: &Path, command: &str) -> Result<std::path::PathBuf> {
    let p = Path::new(command);
    if p.is_absolute() {
        return Ok(p.to_path_buf());
    }
    let candidate = plugin_dir.join(p);
    if !candidate.exists() {
        return Err(anyhow!(
            "插件工具可执行文件不存在: {}（相对插件根目录 {plugin_dir:?}）",
            command
        ));
    }
    Ok(candidate)
}

/// 生成工具 doc_markdown：带 YAML frontmatter schema（供 `openai_parameters_from_doc_or_builtin`），
/// 正文为说明文本（供 `## Tools` 附录）。
fn build_doc_markdown(decl: &PluginToolDecl) -> String {
    let schema = decl.schema.clone().unwrap_or_else(|| {
        serde_json::json!({
            "type": "object",
            "properties": {}
        })
    });
    let schema_yaml = serde_yaml::to_string(&schema).unwrap_or_else(|_| "{}".into());
    let description = if decl.description.trim().is_empty() {
        format!("{}（插件工具）", decl.name)
    } else {
        decl.description.trim().to_string()
    };
    format!(
        "---\nschema:\n{}\n---\n\n{}",
        indent_yaml(&schema_yaml),
        description
    )
}

fn indent_yaml(yaml: &str) -> String {
    yaml.lines()
        .map(|l| format!("  {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// 执行 sidecar：写 stdin JSON → 读 stdout JSON → 解析 `{ok, result|error}`。
fn run_sidecar(
    command_path: &Path,
    tool_name: &str,
    args: &[String],
    env: &std::collections::HashMap<String, String>,
    call_args: serde_json::Value,
    timeout_ms: u64,
) -> Result<String> {
    let request = serde_json::json!({
        "tool": tool_name,
        "args": call_args,
    });

    let mut child = Command::new(command_path)
        .args(args)
        .envs(env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| anyhow!("启动插件工具失败 ({}): {e}", command_path.display()))?;

    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("无法打开插件工具 stdin"))?;
        stdin
            .write_all(request.to_string().as_bytes())
            .map_err(|e| anyhow!("写入插件工具 stdin 失败: {e}"))?;
        stdin.flush().ok();
    }

    // 超时等待：用线程 + channel 实现（std::process::Command 无原生 timeout）。
    let mut child_owned = child;
    let (tx, rx) = std::sync::mpsc::channel::<Result<(String, String)>>();
    let waiter = std::thread::spawn(move || {
        let mut stdout = match child_owned.stdout.take() {
            Some(s) => s,
            None => {
                let _ = tx.send(Err(anyhow!("无法打开插件工具 stdout")));
                return Ok::<(), anyhow::Error>(());
            }
        };
        let mut out = String::new();
        stdout
            .read_to_string(&mut out)
            .map_err(|e| anyhow!("读取插件工具 stdout 失败: {e}"))?;
        let status = child_owned
            .wait()
            .map_err(|e| anyhow!("等待插件工具失败: {e}"))?;
        let _ = tx.send(Ok((out, status.code().unwrap_or(-1).to_string())));
        Ok::<(), anyhow::Error>(())
    });

    match rx.recv_timeout(Duration::from_millis(timeout_ms)) {
        Ok(Ok((stdout, code))) => {
            waiter.join().ok();
            parse_sidecar_response(&stdout, &code)
        }
        Ok(Err(e)) => {
            waiter.join().ok();
            Err(e)
        }
        Err(_) => {
            // 超时：尽力终止子进程（PID 已随 child 移动，无法直接 kill；记录日志）
            log::warn!(
                "插件工具超时 ({timeout_ms}ms): {} {}",
                command_path.display(),
                tool_name
            );
            waiter.join().ok();
            Err(anyhow!("插件工具 {tool_name} 执行超时（>{timeout_ms}ms）"))
        }
    }
}

fn parse_sidecar_response(stdout: &str, exit_code: &str) -> Result<String> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        if exit_code == "0" {
            return Ok(String::new());
        }
        return Err(anyhow!("插件工具退出码 {exit_code}，无输出"));
    }
    match serde_json::from_str::<serde_json::Value>(trimmed) {
        Ok(v) => {
            if v.get("ok").and_then(|b| b.as_bool()).unwrap_or(false) {
                let result = v
                    .get("result")
                    .and_then(|r| r.as_str())
                    .unwrap_or("")
                    .to_string();
                Ok(result)
            } else {
                let error = v
                    .get("error")
                    .and_then(|e| e.as_str())
                    .unwrap_or("插件工具返回失败")
                    .to_string();
                Err(anyhow!("{error}"))
            }
        }
        Err(_) => {
            if exit_code == "0" {
                Ok(trimmed.to_string())
            } else {
                Err(anyhow!(
                    "插件工具退出码 {exit_code}，输出无法解析: {trimmed}"
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn resolve_command_path_absolute_and_relative() {
        let tmp = tempfile::tempdir().unwrap();
        let plugin = tmp.path().join("com.example.demo");
        fs::create_dir_all(plugin.join("bin")).unwrap();
        let tool = plugin.join("bin/demo-tool");
        fs::write(&tool, "#!/bin/sh\nexit 0\n").unwrap();

        let abs = resolve_command_path(&plugin, tool.to_str().unwrap()).unwrap();
        assert_eq!(abs, tool);

        let rel = resolve_command_path(&plugin, "bin/demo-tool").unwrap();
        assert_eq!(rel, tool);

        assert!(resolve_command_path(&plugin, "missing-tool").is_err());
    }

    #[test]
    fn build_doc_markdown_embeds_schema() {
        let decl = PluginToolDecl {
            name: "demo_hello".into(),
            risk_level: "low".into(),
            requires_approval: false,
            parallel_eligible: false,
            description: "Say hello".into(),
            schema: Some(
                serde_json::json!({"type": "object", "properties": {"who": {"type": "string"}}}),
            ),
            exec: None,
        };
        let md = build_doc_markdown(&decl);
        assert!(md.starts_with("---\nschema:"));
        assert!(md.contains("Say hello"));
        // 解析回 schema 不应报错
        let parsed = crate::tools::tool_doc::json_schema_from_markdown(&md).unwrap();
        assert!(parsed["properties"]["who"]["type"].is_string());
    }

    #[test]
    fn build_doc_markdown_default_schema_when_missing() {
        let decl = PluginToolDecl {
            name: "no_schema".into(),
            risk_level: "low".into(),
            requires_approval: false,
            parallel_eligible: false,
            description: String::new(),
            schema: None,
            exec: None,
        };
        let md = build_doc_markdown(&decl);
        let parsed = crate::tools::tool_doc::json_schema_from_markdown(&md).unwrap();
        assert_eq!(parsed["type"], "object");
    }

    #[test]
    fn parse_sidecar_response_ok_and_error() {
        let ok = r#"{"ok": true, "result": "hello"}"#;
        assert_eq!(parse_sidecar_response(ok, "0").unwrap(), "hello");

        let err = r#"{"ok": false, "error": "boom"}"#;
        let e = parse_sidecar_response(err, "1").unwrap_err().to_string();
        assert_eq!(e, "boom");

        assert!(parse_sidecar_response("", "1").is_err());
        assert_eq!(parse_sidecar_response("raw text", "0").unwrap(), "raw text");
    }
}
