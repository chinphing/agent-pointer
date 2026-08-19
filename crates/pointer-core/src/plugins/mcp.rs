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
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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

/// 一个已连接的 MCP server 会话（共享引用；`shutdown` 关闭子进程/连接）。
pub struct McpClient {
    plugin_id: String,
    server_name: String,
    next_id: AtomicU64,
    transport: Transport,
    pending: Arc<Mutex<HashMap<u64, std::sync::mpsc::Sender<Value>>>>,
}

enum Transport {
    /// stdio：本地子进程（插件自带 / 本机命令）。
    Stdio {
        stdin: Mutex<ChildStdin>,
        child: Arc<Mutex<Option<Child>>>,
        /// 存活标记：读线程在 stdout EOF（进程退出）时置 false。
        alive: Arc<AtomicBool>,
    },
    /// streamable HTTP：远程服务（客户端连接别人提供的 MCP server）。
    /// 注意：不在字段里缓存 blocking client——blocking client 内部持有 tokio
    /// runtime，在 tokio 异步上下文创建会 panic（服务端 axum / 测试）。请求时
    /// 在独立线程内创建/释放（工具调用频率低，可接受）。
    Http {
        url: String,
        headers: HeaderMap,
        /// 服务器在 initialize 响应头下发的 `Mcp-Session-Id`（有则后续请求回传）。
        session_id: Mutex<Option<String>>,
        /// initialize 协商出的协议版本（后续请求回传 `MCP-Protocol-Version`）。
        protocol_version: Mutex<Option<String>>,
        alive: Arc<AtomicBool>,
    },
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
            transport: Transport::Stdio {
                stdin: Mutex::new(stdin),
                child: Arc::new(Mutex::new(Some(child))),
                alive: Arc::new(AtomicBool::new(true)),
            },
            pending: Arc::new(Mutex::new(HashMap::new())),
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

    /// 连接远程 MCP 服务（streamable HTTP，客户端场景）。
    /// `decl.url` 必填；`decl.headers` 附加请求头（如 Authorization）。
    pub fn connect_http(plugin_id: &str, decl: &McpServerDecl) -> Result<Arc<McpClient>> {
        let url = decl
            .url
            .clone()
            .ok_or_else(|| anyhow!("MCP server {}: http 传输缺少 url", decl.name))?;
        let mut headers = HeaderMap::new();
        if let Some(hs) = &decl.headers {
            for (k, v) in hs {
                let name = HeaderName::from_bytes(k.as_bytes())
                    .map_err(|_| anyhow!("MCP server {}: 非法请求头名 {k}", decl.name))?;
                let value = HeaderValue::from_str(v)
                    .map_err(|_| anyhow!("MCP server {}: 非法请求头值 {v}", decl.name))?;
                headers.insert(name, value);
            }
        }

        let mcp = Arc::new(McpClient {
            plugin_id: plugin_id.to_string(),
            server_name: decl.name.clone(),
            next_id: AtomicU64::new(1),
            transport: Transport::Http {
                url,
                headers,
                session_id: Mutex::new(None),
                protocol_version: Mutex::new(None),
                alive: Arc::new(AtomicBool::new(true)),
            },
            pending: Arc::new(Mutex::new(HashMap::new())),
        });

        // initialize 握手
        let init_result: Value = mcp
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
        let negotiated = init_result
            .get("protocolVersion")
            .and_then(|v| v.as_str())
            .unwrap_or(MCP_PROTOCOL_VERSION)
            .to_string();
        // 记录协商版本，后续 HTTP 请求回传 MCP-Protocol-Version 头。
        if let Transport::Http {
            protocol_version, ..
        } = &mcp.transport
        {
            *protocol_version.lock() = Some(negotiated.clone());
        }
        log::info!(
            "plugin {}: MCP server `{}` HTTP 握手成功 (protocol={negotiated})",
            plugin_id,
            decl.name
        );
        mcp.notify("notifications/initialized", json!({}))?;
        Ok(mcp)
    }

    fn spawn_reader(&self, stdout: ChildStdout) {
        let Transport::Stdio { alive, .. } = &self.transport else {
            return;
        };
        let alive = alive.clone();
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
            // stdout 关闭：进程已退出，清理所有 pending 并置死标记（watchdog 据此重启）。
            alive.store(false, Ordering::SeqCst);
            let pending = pending.lock();
            for (_, tx) in pending.iter() {
                let _ = tx.send(json!({"error": {"code": -32000, "message": "MCP server 已退出"}}));
            }
        });
    }

    /// 发送 JSON-RPC 请求并等待带 `id` 的响应（带超时；纯同步，可安全用于 tokio 线程）。
    /// stdio 走子进程管道；http 走 streamable HTTP POST。
    pub fn request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let payload = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        match &self.transport {
            Transport::Stdio { stdin, .. } => {
                let (tx, rx) = std::sync::mpsc::channel();
                self.pending.lock().insert(id, tx);
                {
                    let mut stdin = stdin.lock();
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
            Transport::Http { alive, .. } => {
                let result = self.http_post(&payload, timeout);
                if result.is_err() {
                    alive.store(false, Ordering::SeqCst);
                }
                result.and_then(parse_rpc_response)
            }
        }
    }

    /// streamable HTTP 单次 POST：附加 `Mcp-Session-Id` / `MCP-Protocol-Version`
    /// 头（会话协商后），并在 2xx 响应中保存服务器下发的 `Mcp-Session-Id`。
    /// streamable HTTP 单次 POST。reqwest blocking client 内部持有 tokio runtime，
    /// 若在 tokio 运行时线程内创建/释放会 panic（`Cannot drop a runtime...`），
    /// 而工具 handler 恰好可能在 tokio 线程执行——因此整个请求在独立线程内
    /// 完成（创建/释放 client 都在非 tokio 线程），调用方阻塞等待结果。
    fn http_post_once(
        url: &str,
        user_headers: &HeaderMap,
        payload: &Value,
        session_id: Option<&str>,
        protocol_version: Option<&str>,
        timeout: Duration,
    ) -> Result<HttpRawResponse> {
        let url = url.to_string();
        let user_headers = user_headers.clone();
        let payload = payload.clone();
        let sid = session_id.map(|s| s.to_string());
        let ver = protocol_version.map(|s| s.to_string());
        let (tx, rx) = std::sync::mpsc::channel::<Result<HttpRawResponse>>();
        std::thread::spawn(move || {
            let _ = tx.send(Self::http_post_once_inner(
                &url,
                &user_headers,
                &payload,
                sid.as_deref(),
                ver.as_deref(),
                timeout,
            ));
        });
        rx.recv()
            .map_err(|_| anyhow!("MCP HTTP 请求线程意外退出"))?
    }

    fn http_post_once_inner(
        url: &str,
        user_headers: &HeaderMap,
        payload: &Value,
        session_id: Option<&str>,
        protocol_version: Option<&str>,
        timeout: Duration,
    ) -> Result<HttpRawResponse> {
        let method_str = payload
            .get("method")
            .and_then(|m| m.as_str())
            .unwrap_or("?");
        let client = reqwest::blocking::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| anyhow!("创建 HTTP 客户端失败: {e}"))?;
        let mut req = client
            .post(url)
            .headers(user_headers.clone())
            .header(reqwest::header::ACCEPT, "application/json, text/event-stream")
            .json(payload);
        if let Some(sid) = session_id {
            req = req.header("Mcp-Session-Id", sid);
        }
        if let Some(ver) = protocol_version {
            req = req.header("MCP-Protocol-Version", ver);
        }
        let resp = req
            .send()
            .map_err(|e| anyhow!("MCP HTTP 请求 {method_str} 失败: {e}"))?;
        let status = resp.status().as_u16();
        let new_session = resp
            .headers()
            .get("Mcp-Session-Id")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let location = resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let text = resp
            .text()
            .map_err(|e| anyhow!("读取 MCP HTTP 响应失败: {e}"))?;
        if status == 202 {
            // streamable HTTP：服务端以 202 Accepted 声明异步结果，须 GET Location 拉取 SSE 事件流。
            let Some(loc) = location else {
                return Err(anyhow!(
                    "MCP HTTP 请求 {method_str} 返回 202 Accepted 但缺少 Location 头"
                ));
            };
            let mut get_req = client
                .get(&loc)
                .header(reqwest::header::ACCEPT, "text/event-stream");
            if let Some(sid) = session_id {
                get_req = get_req.header("Mcp-Session-Id", sid);
            }
            let get_resp = get_req.send().map_err(|e| {
                anyhow!("MCP HTTP 请求 {method_str} 202 后 GET 结果流失败: {e}")
            })?;
            let stream_text = get_resp
                .text()
                .map_err(|e| anyhow!("读取 MCP HTTP 202 结果流失败: {e}"))?;
            return Ok(HttpRawResponse {
                status,
                session_id: new_session,
                text: stream_text,
            });
        }
        if status != 404 && !(200..300).contains(&status) {
            return Err(anyhow!(
                "MCP HTTP 请求 {method_str} 返回 {status}: {}",
                text.chars().take(300).collect::<String>()
            ));
        }
        Ok(HttpRawResponse {
            status,
            session_id: new_session,
            text,
        })
    }

    /// HTTP 传输 POST 一条 JSON-RPC 消息并解析响应（streamable HTTP）。
    /// 带 `Mcp-Session-Id`（服务器下发过则回传）与 `MCP-Protocol-Version`
    /// （initialize 协商版本）。收到 404（会话失效）且非 initialize 时，
    /// 自动清空会话并重新握手，然后重试原请求一次。
    fn http_post(&self, payload: &Value, timeout: Duration) -> Result<Value> {
        let Transport::Http {
            url,
            headers,
            session_id,
            protocol_version,
            ..
        } = &self.transport
        else {
            return Err(anyhow!("http_post 仅用于 HTTP 传输"));
        };
        let url = url.clone();
        let headers = headers.clone();

        let mut attempt = 0u32;
        loop {
            attempt += 1;
            let sid = session_id.lock().clone();
            let ver = protocol_version.lock().clone();
            let raw = Self::http_post_once(
                &url,
                &headers,
                payload,
                sid.as_deref(),
                ver.as_deref(),
                timeout,
            )?;
            // 2xx：保存服务器下发的 session id（若有），解析响应。
            if raw.status != 404 {
                if let Some(new_sid) = raw.session_id {
                    *session_id.lock() = Some(new_sid);
                }
                return parse_http_mcp_response(&raw.text);
            }
            // 404：会话失效。initialize 本身 404 无会话可重连，直接报错。
            if payload["method"] == "initialize" {
                return Err(anyhow!(
                    "MCP HTTP initialize 返回 404: {}",
                    raw.text.chars().take(300).collect::<String>()
                ));
            }
            if attempt > 1 {
                return Err(anyhow!(
                    "MCP HTTP 请求 {} 重连后仍返回 404: {}",
                    payload["method"],
                    raw.text.chars().take(300).collect::<String>()
                ));
            }
            log::warn!("MCP HTTP 请求 {} 返回 404，会话失效，重新握手", payload["method"]);
            *session_id.lock() = None;
            self.reinitialize_http(timeout)?;
        }
    }

    /// 会话失效后重新执行 initialize 握手（不含 notifications/initialized，
    /// 由调用方在完成后续请求前补发；此处仅更新会话与协议版本）。
    fn reinitialize_http(&self, timeout: Duration) -> Result<()> {
        let init_payload = json!({
            "jsonrpc": "2.0",
            "id": self.next_id.fetch_add(1, Ordering::SeqCst),
            "method": "initialize",
            "params": {
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": { "name": "pointer", "version": env!("CARGO_PKG_VERSION") },
            },
        });
        let init_result = self.http_post(&init_payload, timeout).context("MCP 重新 initialize 失败")?;
        let negotiated = init_result
            .get("protocolVersion")
            .and_then(|v| v.as_str())
            .unwrap_or(MCP_PROTOCOL_VERSION)
            .to_string();
        if let Transport::Http {
            protocol_version, ..
        } = &self.transport
        {
            *protocol_version.lock() = Some(negotiated.clone());
        }
        log::info!("MCP HTTP 会话已重建 (protocol={negotiated})");
        // 新会话初始化完成后补发 initialized 通知。
        self.notify("notifications/initialized", json!({}))?;
        Ok(())
    }

    /// 发送 JSON-RPC 通知（无 id，不等待响应）。
    pub fn notify(&self, method: &str, params: Value) -> Result<()> {
        let payload = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        match &self.transport {
            Transport::Stdio { stdin, .. } => {
                let mut stdin = stdin.lock();
                serde_json::to_writer(&mut *stdin, &payload)
                    .map_err(|e| anyhow!("写入 MCP 通知失败: {e}"))?;
                writeln!(stdin).map_err(|e| anyhow!("写入 MCP 通知换行失败: {e}"))?;
                stdin.flush().map_err(|e| anyhow!("刷新 MCP stdin 失败: {e}"))?;
                Ok(())
            }
            Transport::Http {
                url,
                headers,
                session_id,
                protocol_version,
                ..
            } => {
                // 通知无 id，发完即弃（独立线程内发，避免 async 上下文建 blocking client）
                let url = url.clone();
                let headers = headers.clone();
                let sid = session_id.lock().clone();
                let ver = protocol_version.lock().clone();
                std::thread::spawn(move || {
                    let client = match reqwest::blocking::Client::builder().build() {
                        Ok(c) => c,
                        Err(_) => return,
                    };
                    let mut req = client.post(&url).headers(headers).json(&payload);
                    if let Some(s) = &sid {
                        req = req.header("Mcp-Session-Id", s);
                    }
                    if let Some(v) = &ver {
                        req = req.header("MCP-Protocol-Version", v);
                    }
                    let _ = req.send();
                });
                Ok(())
            }
        }
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
        match &self.transport {
            Transport::Stdio { child, .. } => {
                let mut guard = child.lock();
                if let Some(mut child) = guard.take() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
            Transport::Http { alive, .. } => {
                alive.store(false, Ordering::SeqCst);
            }
        }
    }

    /// 存活探测：stdio 用读线程 EOF 标记 + `try_wait` 交叉验证；http 用请求失败标记。
    pub fn is_alive(&self) -> bool {
        match &self.transport {
            Transport::Stdio { child, alive, .. } => {
                if !alive.load(Ordering::SeqCst) {
                    return false;
                }
                let mut guard = child.lock();
                let exited = match guard.as_mut() {
                    Some(child) => match child.try_wait() {
                        Ok(Some(_)) => true,
                        Ok(None) => false,
                        Err(_) => true,
                    },
                    None => true,
                };
                if exited {
                    alive.store(false, Ordering::SeqCst);
                }
                !exited
            }
            Transport::Http { alive, .. } => alive.load(Ordering::SeqCst),
        }
    }

    /// server 名（状态展示用）。
    pub fn server_name(&self) -> &str {
        &self.server_name
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

/// HTTP 传输单次 POST 的原始响应（状态码 + 服务器下发的 session id + body）。
struct HttpRawResponse {
    status: u16,
    session_id: Option<String>,
    text: String,
}

/// 解析 streamable HTTP 响应：`application/json` 直接解析；
/// `text/event-stream` 取含 `id` 的 `data:` 事件（忽略通知/心跳）。
/// 兼容两种 SSE 形态：整段以 `data:` 开头（简化实现），或标准 SSE 以
/// `event:` 开头（Context7 等远程服务，`data:` 在事件名之后）。
fn parse_http_mcp_response(text: &str) -> Result<Value> {
    let trimmed = text.trim();
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        return Ok(v);
    }
    let mut last = None;
    for line in trimmed.lines() {
        let line = line.trim();
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(data) {
            if v.get("id").is_some() {
                last = Some(v);
            }
        }
    }
    last.ok_or_else(|| anyhow!("MCP HTTP SSE 响应中没有带 id 的数据事件"))
}

/// MCP 连续重启失败达到该次数后进入 `degraded`（停止自动重启，等用户手动重开）。
pub const MAX_MCP_RESTART_FAILURES: u32 = 5;

/// 全局（非插件）MCP 会话在 [`McpSessionManager`] 中的保留 key。
/// 工具注册命名 `mcp.<server>.<tool>`（与插件裸工具名区分），doc_source
/// `plugin:__global__:mcp:{server}:{tool}`（复用 mcp_tool_meta / unregister_mcp_by_plugin）。
pub const GLOBAL_MCP_KEY: &str = "__global__";

/// 单个 MCP server 的运行状态。
#[derive(Debug, Clone, Default)]
pub struct McpServerStatus {
    pub server_name: String,
    pub healthy: bool,
}

/// 插件级 MCP 运行状态（watchdog 维护，供 UI degraded 展示）。
#[derive(Debug, Clone, Default)]
pub struct PluginMcpStatus {
    pub servers: Vec<McpServerStatus>,
    /// 连续重启失败次数（指数退避）。
    pub restart_count: u32,
    pub last_error: Option<String>,
    /// 已达重试上限，停止自动重启（UI 显示 degraded）。
    pub degraded: bool,
    /// 下次重试时间（unix ms；0 = 立即）。
    pub next_attempt_ms: u64,
}

impl PluginMcpStatus {
    fn backoff_ms(n: u32) -> u64 {
        // 1s → 2s → 4s → 8s → 16s → 30s（封顶）
        (1_000u64 << n.min(5)).min(30_000)
    }
}

/// MCP 会话管理器：按插件 id 持有已连接的 client，禁用/卸载时关闭；
/// watchdog 通过状态表驱动崩溃重启与 degraded 展示。
#[derive(Default)]
pub struct McpSessionManager {
    sessions: RwLock<HashMap<String, Vec<Arc<McpClient>>>>,
    statuses: RwLock<HashMap<String, PluginMcpStatus>>,
}

impl McpSessionManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, plugin_id: &str, clients: Vec<Arc<McpClient>>) {
        let servers = clients
            .iter()
            .map(|c| McpServerStatus {
                server_name: c.server_name().to_string(),
                healthy: true,
            })
            .collect();
        self.sessions
            .write()
            .insert(plugin_id.to_string(), clients);
        self.statuses.write().insert(
            plugin_id.to_string(),
            PluginMcpStatus {
                servers,
                ..Default::default()
            },
        );
    }

    /// 是否已有该插件会话（避免重复启动子进程）。
    pub fn has_session(&self, plugin_id: &str) -> bool {
        self.sessions.read().contains_key(plugin_id)
    }

    /// 关闭某插件全部 MCP 会话并移除记录（幂等）。
    pub fn shutdown_plugin(&self, plugin_id: &str) -> usize {
        let removed = self.sessions.write().remove(plugin_id);
        self.statuses.write().remove(plugin_id);
        let n = removed.as_ref().map(|v| v.len()).unwrap_or(0);
        if let Some(clients) = removed {
            for c in &clients {
                c.kill_child();
            }
        }
        n
    }

    /// 插件任一 MCP server 存活（watchdog 崩溃探测）。
    pub fn any_alive(&self, plugin_id: &str) -> bool {
        let g = self.sessions.read();
        let Some(clients) = g.get(plugin_id) else {
            return false;
        };
        clients.iter().any(|c| c.is_alive())
    }

    /// 插件 MCP 运行状态快照。
    pub fn status(&self, plugin_id: &str) -> Option<PluginMcpStatus> {
        self.statuses.read().get(plugin_id).cloned()
    }

    /// 插件 MCP 是否已进入 degraded（重试达上限，UI 显示）。
    pub fn is_degraded(&self, plugin_id: &str) -> bool {
        self.statuses
            .read()
            .get(plugin_id)
            .is_some_and(|s| s.degraded)
    }

    /// 记录一次重启/启动失败：指数退避 + 达上限 degraded。
    pub fn mark_failure(&self, plugin_id: &str, error: &str, now_ms: u64) {
        let mut g = self.statuses.write();
        let st = g.entry(plugin_id.to_string()).or_default();
        st.restart_count += 1;
        st.last_error = Some(error.to_string());
        st.next_attempt_ms = now_ms + PluginMcpStatus::backoff_ms(st.restart_count);
        st.degraded = st.restart_count >= MAX_MCP_RESTART_FAILURES;
    }
}

/// 启动插件声明的全部 MCP server 并注册其工具到 ToolRegistry。
/// 返回已连接 client（由调用方存入 [`McpSessionManager`]）。
pub fn activate_mcp_servers(
    tools: &crate::tools::ToolRegistry,
    record: &PluginRecord,
) -> Result<Vec<Arc<McpClient>>> {
    activate_mcp_servers_with(
        tools,
        &record.id,
        &record.dir,
        &record.manifest.mcp_servers.server,
    )
}

/// P2b：启动全局（非插件）MCP server（来自 pointer-server.toml）。
/// 相对 command 以配置文件所在目录（base_dir）为基准。
pub fn activate_global_mcp_servers(
    tools: &crate::tools::ToolRegistry,
    decls: &[McpServerDecl],
    base_dir: &Path,
) -> Result<Vec<Arc<McpClient>>> {
    activate_mcp_servers_with(tools, GLOBAL_MCP_KEY, base_dir, decls)
}

fn activate_mcp_servers_with(
    tools: &crate::tools::ToolRegistry,
    plugin_id: &str,
    base_dir: &Path,
    decls: &[McpServerDecl],
) -> Result<Vec<Arc<McpClient>>> {
    let mut clients = Vec::new();
    for decl in decls {
        let client = if decl.transport == "http" {
            McpClient::connect_http(plugin_id, decl)
        } else {
            McpClient::connect_stdio(plugin_id, base_dir, decl)
        };
        match client {
            Ok(client) => match client.list_tools() {
                Ok(tool_infos) => {
                    for info in tool_infos {
                        register_mcp_tool(tools, plugin_id, decl, &client, info);
                    }
                    clients.push(client);
                }
                Err(e) => {
                    // 单个 server 的 tools/list 失败只跳过它，不影响其余 server 装配。
                    log::warn!(
                        "MCP ({plugin_id}) server `{}` tools/list 失败，跳过: {e:#}",
                        decl.name
                    );
                }
            },
            Err(e) => {
                log::warn!(
                    "MCP ({plugin_id}) server `{}` 启动失败，跳过: {e:#}",
                    decl.name
                );
            }
        }
    }
    Ok(clients)
}

/// 把 `mcp.<server>.<tool>` 转成 OpenAI 兼容的 wire 工具名（`^[a-zA-Z0-9_-]+$`）。
/// 非 ASCII（中文 server 名）、点号、空格等统一转为 `_` 并压缩连续下划线，
/// 保留 ASCII 可读片段（如 `query-docs`）；handler 仍用原始 server/tool 调用。
fn sanitize_mcp_wire_name(server: &str, tool: &str) -> String {
    let raw = format!("mcp.{server}.{tool}");
    let mut out = String::with_capacity(raw.len());
    let mut pending_underscore = false;
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            if pending_underscore && !out.is_empty() && !out.ends_with('_') {
                out.push('_');
            }
            pending_underscore = false;
            out.push(c);
        } else {
            pending_underscore = true;
        }
    }
    while out.ends_with('_') {
        out.pop();
    }
    if out.is_empty() {
        // 极端情况：server 与 tool 全部非 ASCII
        format!("mcp_tool_{}", tool.len())
    } else {
        out
    }
}

fn register_mcp_tool(
    tools: &crate::tools::ToolRegistry,
    plugin_id: &str,
    decl: &McpServerDecl,
    client: &Arc<McpClient>,
    info: McpToolInfo,
) {
    if info.name.trim().is_empty() {
        return;
    }
    let client = client.clone();
    let tool_name = info.name.clone();
    // 格式 `plugin:{id}:mcp:{server}:{tool}`：`:mcp:` 标识 MCP 工具（重启时
    // 精确注销），server 供 McpRequest Span 埋点解析。
    let doc_source: std::borrow::Cow<'static, str> = std::borrow::Cow::Owned(format!(
        "plugin:{plugin_id}:mcp:{}:{}",
        decl.name, tool_name
    ));
    let description = if info.description.trim().is_empty() {
        format!("{tool_name}（MCP {server}）", server = decl.name)
    } else {
        info.description.trim().to_string()
    };
    // 全局 MCP 用 `mcp.<server>.<tool>` 前缀命名（与插件裸名区分；计划 §6.1），
    // 但必须转成 OpenAI 兼容的 wire 名（`^[a-zA-Z0-9_-]+$`，禁止点号/中文）。
    let entry_name = if plugin_id == GLOBAL_MCP_KEY {
        sanitize_mcp_wire_name(&decl.name, &tool_name)
    } else {
        tool_name.clone()
    };
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
    .with_plugin_id(plugin_id.to_string());
    if let Some(schema) = info.input_schema {
        entry = entry.with_schema(schema);
    }
    tools.register(entry);
    log::info!(
        "MCP ({plugin_id}) 工具已注册（server `{}`）",
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
            url: None,
            headers: None,
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

    #[test]
    fn parse_http_mcp_response_handles_plain_json() {
        // application/json 响应：直接解析。
        let body = "{\"result\":{\"tools\":[]},\"jsonrpc\":\"2.0\",\"id\":2}";
        let v = parse_http_mcp_response(body).expect("parse");
        assert_eq!(v["id"], 2);
    }

    #[test]
    fn parse_http_mcp_response_handles_sse_data_line() {
        // 简化 SSE：整段以 data: 开头（历史兼容）。
        let body = "data: {\"result\":{\"tools\":[]},\"jsonrpc\":\"2.0\",\"id\":2}\n";
        let v = parse_http_mcp_response(body).expect("parse");
        assert_eq!(v["id"], 2);
    }

    #[test]
    fn parse_http_mcp_response_handles_sse_event_line() {
        // 标准 SSE（Context7 等远程 MCP）：以 event: message 开头，data: 携带响应。
        let body =
            "event: message\ndata: {\"result\":{\"protocolVersion\":\"2024-11-05\"},\"jsonrpc\":\"2.0\",\"id\":1}\n";
        let v = parse_http_mcp_response(body).expect("parse");
        assert_eq!(v["id"], 1);
        assert_eq!(v["result"]["protocolVersion"], "2024-11-05");
    }

    // ---- streamable HTTP：会话头回传 / 404 重连 ----

    /// 最小同步 HTTP mock：每个请求调用 handler 返回 (status, headers, body)，
    /// 并记录 (method, headers, raw_text) 供断言。返回 (base_url, seen)。
    fn spawn_http_mock(
        handler: Arc<
            dyn Fn(&str, &HashMap<String, String>, &str) -> (u16, Vec<(String, String)>, String)
                + Send
                + Sync,
        >,
    ) -> (
        String,
        Arc<Mutex<Vec<(String, HashMap<String, String>, String)>>>,
    ) {
        use std::io::Read as _;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let seen: Arc<Mutex<Vec<(String, HashMap<String, String>, String)>>> =
            Arc::new(Mutex::new(Vec::new()));
        let seen2 = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut buf = Vec::new();
                let mut tmp = [0u8; 4096];
                loop {
                    let n = match stream.read(&mut tmp) {
                        Ok(0) => break,
                        Ok(n) => n,
                        Err(_) => break,
                    };
                    buf.extend_from_slice(&tmp[..n]);
                    if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                let Ok(text) = String::from_utf8(buf) else {
                    continue;
                };
                let mut lines = text.split("\r\n");
                let method = lines.next().unwrap_or_default().to_string();
                let mut headers = HashMap::new();
                let mut content_length = 0usize;
                for line in lines {
                    if line.is_empty() {
                        break;
                    }
                    if let Some((k, v)) = line.split_once(':') {
                        headers.insert(k.trim().to_lowercase(), v.trim().to_string());
                        if k.trim().eq_ignore_ascii_case("content-length") {
                            content_length = v.trim().parse().unwrap_or(0);
                        }
                    }
                }
                // 按 Content-Length 截取纯请求体（JSON-RPC 请求体单行 JSON，无 \r\n 干扰）
                let body_start = text
                    .find("\r\n\r\n")
                    .map(|i| i + 4)
                    .unwrap_or(text.len())
                    .min(text.len());
                let body = text[body_start..].chars().take(content_length).collect::<String>();
                let (status, hdrs, resp_body) = handler(&method, &headers, &body);
                let mut resp = format!("HTTP/1.1 {status} X\r\n");
                for (k, v) in &hdrs {
                    resp.push_str(&format!("{k}: {v}\r\n"));
                }
                resp.push_str(&format!(
                    "Content-Length: {}\r\nConnection: close\r\n\r\n",
                    resp_body.len()
                ));
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.write_all(resp_body.as_bytes());
                seen2.lock().push((method, headers, body));
            }
        });
        (format!("http://{addr}/mcp"), seen)
    }

    fn http_decl(url: String) -> McpServerDecl {
        McpServerDecl {
            name: "mock".into(),
            transport: "http".into(),
            command: String::new(),
            args: vec![],
            env: HashMap::new(),
            url: Some(url),
            headers: None,
        }
    }

    #[test]
    fn http_transport_follows_202_location_stream() {
        // streamable HTTP 异步形态：POST 返回 202 + Location，客户端须 GET 拉取 SSE 结果。
        let base_holder: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let base_for_handler = base_holder.clone();
        let handler: Arc<
            dyn Fn(&str, &HashMap<String, String>, &str) -> (u16, Vec<(String, String)>, String)
                + Send
                + Sync,
        > = Arc::new(move |method, _headers, body| {
            if method.starts_with("GET") {
                let is_init = method.contains("/result/init");
                let payload = if is_init {
                    r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"mock","version":"1"}}}"#
                } else {
                    r#"{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"async-tool","description":"From 202 stream","inputSchema":{"type":"object"}}]}}"#
                };
                return (
                    200,
                    vec![("Content-Type".to_string(), "text/event-stream".to_string())],
                    format!("event: message\ndata: {payload}\n"),
                );
            }
            let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
            let m = v.get("method").and_then(|m| m.as_str()).unwrap_or("");
            let base = base_for_handler.lock().clone();
            match m {
                "initialize" => (
                    202,
                    vec![
                        ("Location".to_string(), format!("{base}/result/init")),
                        ("Mcp-Session-Id".to_string(), "sess-202".to_string()),
                    ],
                    String::new(),
                ),
                "tools/list" => (
                    202,
                    vec![("Location".to_string(), format!("{base}/result/list"))],
                    String::new(),
                ),
                _ => (202, vec![], String::new()),
            }
        });
        let (url, seen) = spawn_http_mock(handler);
        *base_holder.lock() = url.clone();
        let client = McpClient::connect_http("t", &http_decl(url)).expect("connect");
        let tools = client.list_tools().expect("list_tools");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "async-tool");
        let seen = seen.lock();
        let get_req = seen
            .iter()
            .find(|(m, _, _)| m.starts_with("GET"))
            .expect("应有 GET /result 拉流请求");
        assert!(get_req.0.contains("result"), "GET 应命中 Location: {}", get_req.0);
    }

    #[test]
    fn activate_skips_server_whose_list_tools_fails() {
        // server A 正常；server B tools/list 返回 500 → 只跳过 B，A 的工具仍注册。
        let handler_a: Arc<
            dyn Fn(&str, &HashMap<String, String>, &str) -> (u16, Vec<(String, String)>, String)
                + Send
                + Sync,
        > = Arc::new(|_method, _headers, body| {
            let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
            let m = v.get("method").and_then(|m| m.as_str()).unwrap_or("");
            match m {
                "initialize" => (
                    200,
                    vec![],
                    r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"A","version":"1"}}}"#.to_string(),
                ),
                "tools/list" => (
                    200,
                    vec![],
                    r#"{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"tool-a","description":"from A","inputSchema":{"type":"object"}}]}}"#.to_string(),
                ),
                _ => (202, vec![], String::new()),
            }
        });
        let handler_b: Arc<
            dyn Fn(&str, &HashMap<String, String>, &str) -> (u16, Vec<(String, String)>, String)
                + Send
                + Sync,
        > = Arc::new(|_method, _headers, body| {
            let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
            let m = v.get("method").and_then(|m| m.as_str()).unwrap_or("");
            match m {
                "initialize" => (
                    200,
                    vec![],
                    r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"B","version":"1"}}}"#.to_string(),
                ),
                "tools/list" => (500, vec![], "boom".to_string()),
                _ => (202, vec![], String::new()),
            }
        });
        let (url_a, _seen_a) = spawn_http_mock(handler_a);
        let (url_b, _seen_b) = spawn_http_mock(handler_b);
        let tools = crate::tools::ToolRegistry::new();
        let mut decl_a = http_decl(url_a);
        decl_a.name = "server-a".into();
        let mut decl_b = http_decl(url_b);
        decl_b.name = "server-b".into();
        let clients = activate_global_mcp_servers(
            &tools,
            &[decl_a, decl_b],
            std::path::Path::new("."),
        )
        .expect("部分 server 失败不应使整批装配失败");
        assert_eq!(clients.len(), 1, "只有 server-a 建立会话");
        let defs = tools.list_defs();
        assert_eq!(defs.len(), 1, "只注册 server-a 的工具");
        assert_eq!(defs[0].name, "mcp_server-a_tool-a");
        clients[0].kill_child();
    }

    #[tokio::test]
    async fn http_requests_safe_inside_tokio_runtime() {
        // 回归：Cannot drop a runtime panic —— 工具 handler 可能在 tokio 运行时线程执行，
        // blocking client 的创建/释放必须发生在非 tokio 线程。
        let handler: Arc<
            dyn Fn(&str, &HashMap<String, String>, &str) -> (u16, Vec<(String, String)>, String)
                + Send
                + Sync,
        > = Arc::new(|_method, _headers, body| {
            let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
            let m = v.get("method").and_then(|m| m.as_str()).unwrap_or("");
            match m {
                "initialize" => (
                    200,
                    vec![],
                    r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"mock","version":"1"}}}"#.to_string(),
                ),
                "tools/list" => (
                    200,
                    vec![],
                    r#"{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"echo","description":"echo","inputSchema":{"type":"object"}}]}}"#.to_string(),
                ),
                "tools/call" => (
                    200,
                    vec![],
                    r#"{"jsonrpc":"2.0","id":3,"result":{"content":[{"type":"text","text":"pong"}]}}"#.to_string(),
                ),
                _ => (202, vec![], String::new()),
            }
        });
        let (url, _seen) = spawn_http_mock(handler);
        // 直接在 tokio 运行时线程执行 MCP 请求（修复前 blocking client drop 会 panic）
        let client = McpClient::connect_http("t", &http_decl(url)).expect("connect");
        let tools = client.list_tools().expect("list_tools");
        assert_eq!(tools.len(), 1);
        let out = client.call_tool("echo", json!({ "x": 1 })).expect("call_tool");
        assert_eq!(out, "pong");
        client.kill_child();
    }

    #[test]
    fn http_transport_echoes_session_and_protocol_headers() {
        let handler: Arc<
            dyn Fn(&str, &HashMap<String, String>, &str) -> (u16, Vec<(String, String)>, String)
                + Send
                + Sync,
        > = Arc::new(|_method, _headers, body| {
            let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
            let m = v.get("method").and_then(|m| m.as_str()).unwrap_or("");
            match m {
                "initialize" => (
                    200,
                    vec![("Mcp-Session-Id".to_string(), "sess-abc".to_string())],
                    r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"mock","version":"1"}}}"#.to_string(),
                ),
                "tools/list" => (
                    200,
                    vec![],
                    r#"{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"echo","description":"echo","inputSchema":{"type":"object"}}]}}"#.to_string(),
                ),
                _ => (202, vec![], String::new()),
            }
        });
        let (url, seen) = spawn_http_mock(handler);
        let client = McpClient::connect_http("t", &http_decl(url)).expect("connect");
        let tools = client.list_tools().expect("list_tools");
        assert_eq!(tools.len(), 1);
        // 等待后台 initialized 通知线程落盘
        std::thread::sleep(Duration::from_millis(300));
        let seen = seen.lock();
        let tool_req = seen
            .iter()
            .find(|(_, _, b)| b.contains("\"tools/list\""))
            .expect("应有 tools/list 请求");
        // 会话与协议版本头必须回传
        assert_eq!(
            tool_req.1.get("mcp-session-id").map(String::as_str),
            Some("sess-abc")
        );
        assert_eq!(
            tool_req.1.get("mcp-protocol-version").map(String::as_str),
            Some("2025-06-18")
        );
        client.kill_child();
    }

    #[test]
    fn sanitize_mcp_wire_name_creates_legal_openai_name() {
        // 中文 server + 点号：转为 ASCII 合法名，保留可读 tool 片段
        let n = sanitize_mcp_wire_name("查询技术文档", "query-docs");
        assert!(n.starts_with("mcp_"), "got {n}");
        assert!(n.ends_with("query-docs"), "got {n}");
        assert!(
            n.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'),
            "非法 wire 名: {n}"
        );
        // 全 ASCII 正常保留
        let n2 = sanitize_mcp_wire_name("my-server", "do_thing");
        assert_eq!(n2, "mcp_my-server_do_thing");
        // 全非 ASCII 兜底仍合法
        let n3 = sanitize_mcp_wire_name("查询", "文档");
        assert!(
            n3.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'),
            "非法 wire 名: {n3}"
        );
        assert!(!n3.is_empty());
    }

    #[test]
    fn global_mcp_register_uses_legal_wire_name_and_calls_original_tool() {
        // mock：initialize + tools/list（name=query-docs）+ tools/call（记录请求）
        let handler: Arc<
            dyn Fn(&str, &HashMap<String, String>, &str) -> (u16, Vec<(String, String)>, String)
                + Send
                + Sync,
        > = Arc::new(|_method, _headers, body| {
            let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
            let m = v.get("method").and_then(|m| m.as_str()).unwrap_or("");
            match m {
                "initialize" => (
                    200,
                    vec![],
                    r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"mock","version":"1"}}}"#.to_string(),
                ),
                "tools/list" => (
                    200,
                    vec![],
                    r#"{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"query-docs","description":"Query docs","inputSchema":{"type":"object"}}]}}"#.to_string(),
                ),
                "tools/call" => (
                    200,
                    vec![],
                    r#"{"jsonrpc":"2.0","id":3,"result":{"content":[{"type":"text","text":"docs!"}]}}"#.to_string(),
                ),
                _ => (202, vec![], String::new()),
            }
        });
        let (url, seen) = spawn_http_mock(handler);
        let tools = crate::tools::ToolRegistry::new();
        let decl = http_decl(url);
        let clients = activate_mcp_servers_with(
            &tools,
            GLOBAL_MCP_KEY,
            std::path::Path::new("."),
            std::slice::from_ref(&decl),
        )
        .expect("activate");
        assert_eq!(clients.len(), 1);

        let defs = tools.list_defs();
        assert_eq!(defs.len(), 1);
        let name = &defs[0].name;
        assert!(
            name.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'),
            "注册的 wire 名非法（OpenAI 会 400）: {name}"
        );

        // OpenAI tools 载荷：description 应保留 MCP 原始描述（模型识别工具用途的唯一依据）
        let wire = tools.openai_tools(&[name.clone()]);
        assert_eq!(wire.len(), 1);
        let desc = wire[0]["function"]["description"].as_str().unwrap_or("");
        assert!(
            desc.contains("Query docs"),
            "MCP 工具描述不应被 compact，got: {desc:?}"
        );
        assert!(
            !desc.contains("parameters in schema"),
            "MCP 工具描述不应是 compact 占位，got: {desc:?}"
        );

        // 通过注册名调用 → 底层应调用原始工具名 query-docs
        tools
            .invoke(name, json!({ "q": "tokio" }))
            .expect("invoke");
        std::thread::sleep(Duration::from_millis(300));
        let seen = seen.lock();
        let call = seen
            .iter()
            .find(|(_, _, b)| b.contains("\"tools/call\""))
            .expect("应有 tools/call 请求");
        assert!(
            call.2.contains("\"name\":\"query-docs\""),
            "handler 应使用原始工具名，got: {}",
            call.2
        );
        clients[0].kill_child();
    }

    #[test]
    fn http_transport_reinitializes_on_404() {
        #[derive(Default)]
        struct St {
            init_count: u32,
            list_count: u32,
        }
        let st = Arc::new(Mutex::new(St::default()));
        let st2 = st.clone();
        let handler: Arc<
            dyn Fn(&str, &HashMap<String, String>, &str) -> (u16, Vec<(String, String)>, String)
                + Send
                + Sync,
        > = Arc::new(move |_method, _headers, body| {
            let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
            let m = v.get("method").and_then(|m| m.as_str()).unwrap_or("");
            let mut st = st2.lock();
            match m {
                "initialize" => {
                    st.init_count += 1;
                    let sid = if st.init_count == 1 { "sess-old" } else { "sess-new" };
                    (
                        200,
                        vec![("Mcp-Session-Id".to_string(), sid.to_string())],
                        r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"mock","version":"1"}}}"#.to_string(),
                    )
                }
                "tools/list" => {
                    st.list_count += 1;
                    if st.list_count == 1 {
                        // 业务请求触发 404：会话失效
                        (404, vec![], String::new())
                    } else {
                        (
                            200,
                            vec![],
                            r#"{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"echo","description":"echo","inputSchema":{"type":"object"}}]}}"#.to_string(),
                        )
                    }
                }
                _ => (202, vec![], String::new()),
            }
        });
        let (url, seen) = spawn_http_mock(handler);
        let client = McpClient::connect_http("t", &http_decl(url)).expect("connect");
        let tools = client.list_tools().expect("404 后自动重连应成功");
        assert_eq!(tools.len(), 1);
        let st = st.lock();
        assert_eq!(st.init_count, 2, "应重新 initialize 一次");
        assert_eq!(st.list_count, 2, "404 后应重试一次");
        drop(st);
        std::thread::sleep(Duration::from_millis(300));
        let seen = seen.lock();
        let last_tools = seen
            .iter()
            .filter(|(_, _, b)| b.contains("\"tools/list\""))
            .last()
            .unwrap();
        assert_eq!(
            last_tools.1.get("mcp-session-id").map(String::as_str),
            Some("sess-new"),
            "重连后应使用新会话 id"
        );
        client.kill_child();
    }
}
