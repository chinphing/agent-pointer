//! 插件 MCP server 接入（P2）：最小 stdio JSON-RPC 2.0 客户端。
//!
//! 启动 manifest `[[mcp_servers.server]]` 声明的子进程（stdio transport），
//! 完成 `initialize` 握手 → `notifications/initialized` → `tools/list` →
//! `tools/call`。后台读线程按 JSON-RPC `id` 分发响应；`tools/call` 支持超时。
//!
//! 生命周期：插件启用时 `connect_stdio` 启动进程并发现工具；禁用/卸载时
//! `McpSessionManager::shutdown_plugin` 关闭进程。MCP 工具与 sidecar 工具
//! 共用 `ToolRegistry` 注册与审批链路。

use crate::plugins::manifest::McpServerDecl;
use crate::plugins::registry::PluginRecord;
use crate::tools::{ToolEntry, ToolHandler};
use anyhow::{anyhow, Context, Result};
use parking_lot::{Mutex, RwLock};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// MCP 协议版本（客户端声明，服务端协商）。
const MCP_PROTOCOL_VERSION: &str = "2024-11-05";
/// tools/call 默认超时（毫秒）。
const DEFAULT_MCP_CALL_TIMEOUT_MS: u64 = 60_000;
/// initialize 握手超时（毫秒）。
const DEFAULT_MCP_HANDSHAKE_TIMEOUT_MS: u64 = 15_000;

/// MCP 工具信息（`tools/list` 结果项）。
#[derive(Debug, Clone)]
pub struct McpToolInfo {
    pub name: String,
    pub description: String,
    pub input_schema: Option<Value>,
}

/// 一个已连接的 MCP server 会话（共享引用；`shutdown` 关闭子进程）。
pub struct McpClient {
    plugin_id: String,
    server_name: String,
    next_id: AtomicU64,
    stdin: Mutex<ChildStdin>,
    pending: Arc<Mutex<HashMap<u64, std::sync::mpsc::Sender<Value>>>>,
    child: Arc<Mutex<Option<Child>>>,
}

impl Drop for McpClient {
    fn drop(&mut self) {
        self.kill_child();
    }
}

impl McpClient {
    /// 解析命令路径：绝对路径直接用；相对路径相对插件根目录。
    fn resolve_command(plugin_dir: &Path, command: &str) -> Result<std::path::PathBuf> {
        let p = Path::new(command);
        if p.is_absolute() {
            return Ok(p.to_path_buf());
        }
        let candidate = plugin_dir.join(p);
        if !candidate.exists() {
            return Err(anyhow!(
                "插件 MCP server 命令不存在: {}（相对插件根目录 {plugin_dir:?}）",
                command
            ));
        }
        Ok(candidate)
    }

    /// 启动 stdio MCP server 并完成握手；失败时清理子进程。
    pub fn connect_stdio(
        plugin_id: &str,
        plugin_dir: &Path,
        decl: &McpServerDecl,
    ) -> Result<Arc<McpClient>> {
        if decl.transport != "stdio" {
            return Err(anyhow!(
                "MCP server {}: transport `{}` 暂不支持（仅 stdio）",
                decl.name,
                decl.transport
            ));
        }
        let command_path = Self::resolve_command(plugin_dir, &decl.command)?;
        let mut child = Command::new(&command_path)
            .args(&decl.args)
            .envs(&decl.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| anyhow!("启动 MCP server 失败 ({}): {e}", command_path.display()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("无法打开 MCP server stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("无法打开 MCP server stdout"))?;

        let client = Arc::new(McpClient {
            plugin_id: plugin_id.to_string(),
            server_name: decl.name.clone(),
            next_id: AtomicU64::new(1),
            stdin: Mutex::new(stdin),
            pending: Arc::new(Mutex::new(HashMap::new())),
            child: Arc::new(Mutex::new(Some(child))),
        });
        client.spawn_reader(stdout);

        // initialize 握手
        let init_result: Value = client
            .request(
                "initialize",
                json!({
                    "protocolVersion": MCP_PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": { "name": "pointer", "version": env!("CARGO_PKG_VERSION") },
                }),
                Duration::from_millis(DEFAULT_MCP_HANDSHAKE_TIMEOUT_MS),
            )
            .context("MCP initialize 握手失败")?;
        if let Some(ver) = init_result.get("protocolVersion").and_then(|v| v.as_str()) {
            log::info!(
                "plugin {}: MCP server `{}` 握手成功 (protocol={ver})",
                plugin_id,
                decl.name
            );
        }
        // notifications/initialized（无 id）
        client.notify("notifications/initialized", json!({}))?;
        Ok(client)
    }

    fn spawn_reader(&self, stdout: ChildStdout) {
        let pending = self.pending.clone();
        let plugin_id = self.plugin_id.clone();
        let server_name = self.server_name.clone();
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                let line = match line {
                    Ok(l) => l,
                    Err(_) => break,
                };
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let parsed: Value = match serde_json::from_str(trimmed) {
                    Ok(v) => v,
                    Err(_) => {
                        log::warn!(
                            "plugin {plugin_id}: MCP server `{server_name}` 输出非 JSON: {trimmed}"
                        );
                        continue;
                    }
                };
                let id = parsed.get("id").and_then(|v| v.as_u64());
                let Some(id) = id else {
                    continue; // 服务端主动通知/日志，忽略
                };
                let tx = pending.lock().remove(&id);
                if let Some(tx) = tx {
                    let _ = tx.send(parsed);
                }
            }
            // stdout 关闭：清理所有 pending
            let pending = pending.lock();
            for (_, tx) in pending.iter() {
                let _ = tx.send(json!({"error": {"code": -32000, "message": "MCP server 已退出"}}));
            }
        });
    }

    /// 发送 JSON-RPC 请求并等待带 `id` 的响应（带超时；纯同步，可安全用于 tokio 线程）。
    pub fn request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = std::sync::mpsc::channel();
        self.pending.lock().insert(id, tx);
        let payload = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        {
            let mut stdin = self.stdin.lock();
            serde_json::to_writer(&mut *stdin, &payload)
                .map_err(|e| anyhow!("写入 MCP 请求失败: {e}"))?;
            writeln!(stdin).map_err(|e| anyhow!("写入 MCP 请求换行失败: {e}"))?;
            stdin.flush().map_err(|e| anyhow!("刷新 MCP stdin 失败: {e}"))?;
        }

        match rx.recv_timeout(timeout) {
            Ok(v) => {
                self.pending.lock().remove(&id);
                parse_rpc_response(v)
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                self.pending.lock().remove(&id);
                Err(anyhow!("MCP 请求 {method} 超时（>{timeout:?}）"))
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                self.pending.lock().remove(&id);
                Err(anyhow!("MCP 请求 {method} 通道关闭（server 退出）"))
            }
        }
    }

    /// 发送 JSON-RPC 通知（无 id，不等待响应）。
    pub fn notify(&self, method: &str, params: Value) -> Result<()> {
        let payload = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        let mut stdin = self.stdin.lock();
        serde_json::to_writer(&mut *stdin, &payload)
            .map_err(|e| anyhow!("写入 MCP 通知失败: {e}"))?;
        writeln!(stdin).map_err(|e| anyhow!("写入 MCP 通知换行失败: {e}"))?;
        stdin.flush().map_err(|e| anyhow!("刷新 MCP stdin 失败: {e}"))?;
        Ok(())
    }

    /// 列出 server 暴露的工具。
    pub fn list_tools(&self) -> Result<Vec<McpToolInfo>> {
        let resp = self.request(
            "tools/list",
            json!({}),
            Duration::from_millis(DEFAULT_MCP_CALL_TIMEOUT_MS),
        )?;
        let tools = resp
            .get("tools")
            .and_then(|t| t.as_array())
            .ok_or_else(|| anyhow!("MCP tools/list 响应缺少 tools 数组: {resp}"))?;
        let mut out = Vec::new();
        for t in tools {
            out.push(McpToolInfo {
                name: t
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("")
                    .to_string(),
                description: t
                    .get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or("")
                    .to_string(),
                input_schema: t.get("inputSchema").cloned(),
            });
        }
        Ok(out)
    }

    /// 调用 MCP 工具，返回文本内容（拼接 content[].text）。
    pub fn call_tool(&self, tool_name: &str, args: Value) -> Result<String> {
        let resp = self.request(
            "tools/call",
            json!({ "name": tool_name, "arguments": args }),
            Duration::from_millis(DEFAULT_MCP_CALL_TIMEOUT_MS),
        )?;
        if resp.get("isError").and_then(|v| v.as_bool()).unwrap_or(false) {
            let err = resp
                .get("content")
                .and_then(|c| c.as_array())
                .map(|arr| extract_text(arr))
                .unwrap_or_else(|| "MCP 工具调用失败".to_string());
            return Err(anyhow!("MCP 工具 {tool_name} 返回错误: {err}"));
        }
        Ok(resp
            .get("content")
            .and_then(|c| c.as_array())
            .map(|arr| extract_text(arr))
            .unwrap_or_default())
    }

    fn kill_child(&self) {
        let mut guard = self.child.lock();
        if let Some(mut child) = guard.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn extract_text(content: &[Value]) -> String {
    content
        .iter()
        .filter_map(|c| c.get("text").and_then(|t| t.as_str()))
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_rpc_response(v: Value) -> Result<Value> {
    if let Some(err) = v.get("error") {
        let code = err.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
        let message = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("未知错误")
            .to_string();
        return Err(anyhow!("MCP 错误 {code}: {message}"));
    }
    Ok(v.get("result").cloned().unwrap_or(Value::Null))
}

/// MCP 会话管理器：按插件 id 持有已连接的 client，禁用/卸载时关闭。
#[derive(Default)]
pub struct McpSessionManager {
    sessions: RwLock<HashMap<String, Vec<Arc<McpClient>>>>,
}

impl McpSessionManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, plugin_id: &str, clients: Vec<Arc<McpClient>>) {
        self.sessions
            .write()
            .insert(plugin_id.to_string(), clients);
    }

    /// 是否已有该插件会话（避免重复启动子进程）。
    pub fn has_session(&self, plugin_id: &str) -> bool {
        self.sessions.read().contains_key(plugin_id)
    }

    /// 关闭某插件全部 MCP 会话并移除记录（幂等）。
    pub fn shutdown_plugin(&self, plugin_id: &str) -> usize {
        let removed = self.sessions.write().remove(plugin_id);
        let n = removed.as_ref().map(|v| v.len()).unwrap_or(0);
        if let Some(clients) = removed {
            for c in &clients {
                c.kill_child();
            }
        }
        n
    }
}

/// 启动插件声明的全部 MCP server 并注册其工具到 ToolRegistry。
/// 返回已连接 client（由调用方存入 [`McpSessionManager`]）。
pub fn activate_mcp_servers(
    tools: &crate::tools::ToolRegistry,
    record: &PluginRecord,
) -> Result<Vec<Arc<McpClient>>> {
    let mut clients = Vec::new();
    for decl in &record.manifest.mcp_servers.server {
        match McpClient::connect_stdio(&record.id, &record.dir, decl) {
            Ok(client) => {
                let tool_infos = client.list_tools()?;
                for info in tool_infos {
                    register_mcp_tool(tools, record, decl, &client, info);
                }
                clients.push(client);
            }
            Err(e) => {
                log::warn!("plugin {}: MCP server `{}` 启动失败，跳过: {e:#}", record.id, decl.name);
            }
        }
    }
    Ok(clients)
}

fn register_mcp_tool(
    tools: &crate::tools::ToolRegistry,
    record: &PluginRecord,
    decl: &McpServerDecl,
    client: &Arc<McpClient>,
    info: McpToolInfo,
) {
    if info.name.trim().is_empty() {
        return;
    }
    let client = client.clone();
    let tool_name = info.name.clone();
    let doc_source: std::borrow::Cow<'static, str> =
        std::borrow::Cow::Owned(format!("plugin:{}:mcp:{}", record.id, tool_name));
    let description = if info.description.trim().is_empty() {
        format!("{tool_name}（MCP {server}）", server = decl.name)
    } else {
        info.description.trim().to_string()
    };
    let entry_name = tool_name.clone();
    let handler: ToolHandler = Arc::new(move |args: Value| -> Result<String> {
        client.call_tool(&tool_name, args)
    });

    let mut entry = ToolEntry::new(
        entry_name,
        doc_source,
        "low",
        false,
        description,
        handler,
    )
    .with_plugin_id(record.id.clone());
    if let Some(schema) = info.input_schema {
        entry = entry.with_schema(schema);
    }
    tools.register(entry);
    log::info!(
        "plugin {}: MCP 工具已注册（server `{}`）",
        record.id,
        decl.name
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::manifest;
    use std::fs;
    use std::path::PathBuf;

    fn write_mcp_plugin(root: &Path, server_json: &str) -> PathBuf {
        let dir = root.join("com.example.mcp");
        fs::create_dir_all(dir.join("bin")).unwrap();
        fs::write(
            dir.join("pointer-plugin.toml"),
            r#"
[plugin]
id = "com.example.mcp"
name = "MCP Demo"
version = "1.0.0"
api_version = "v1"
"#,
        )
        .unwrap();
        fs::write(dir.join("server.json"), server_json).unwrap();
        dir
    }

    fn write_server_script(dir: &Path, name: &str, body: &str) {
        let path = dir.join("bin").join(name);
        fs::write(&path, body).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    #[test]
    fn mcp_connect_list_and_call() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let dir = write_mcp_plugin(root, "{}");
        // 极简 MCP server：bash 逐行读 JSON-RPC，initialize/tools/list/tools/call 各回一条。
        write_server_script(
            &dir,
            "demo-mcp",
            r#"#!/bin/bash
while IFS= read -r line; do
  if echo "$line" | grep -q '"initialize"'; then
    echo '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"demo","version":"1.0"}}}'
  elif echo "$line" | grep -q '"tools/list"'; then
    echo '{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"echo","description":"echo text","inputSchema":{"type":"object","properties":{"text":{"type":"string"}}}}]}}'
  elif echo "$line" | grep -q '"tools/call"'; then
    echo '{"jsonrpc":"2.0","id":3,"result":{"content":[{"type":"text","text":"mcp-echo-ok"}],"isError":false}}'
  fi
done
"#,
        );
        let decl = manifest::parse(
            &fs::read_to_string(dir.join("pointer-plugin.toml")).unwrap(),
        )
        .unwrap();
        let mcp_decl = McpServerDecl {
            name: "demo".into(),
            transport: "stdio".into(),
            command: "bin/demo-mcp".into(),
            args: vec![],
            env: HashMap::new(),
        };

        let client = McpClient::connect_stdio("com.example.mcp", &dir, &mcp_decl)
            .expect("connect");
        let tools = client.list_tools().expect("list tools");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "echo");
        let out = client.call_tool("echo", json!({"text": "hi"})).expect("call");
        assert_eq!(out, "mcp-echo-ok");
        client.kill_child();
        let _ = decl;
    }
}
