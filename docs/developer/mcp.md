# MCP 客户端接入说明（developer）

> 面向**开发者**。讲 Pointer 作为 MCP **客户端**的实现：如何连接 stdio / HTTP 两种
> 传输的 MCP server、配置载体、装配与生命周期、管理 API、测试。
> 用户使用说明见 [`../user/mcp.md`](../user/mcp.md)；插件内 MCP 声明见
> [`../user/plugins.md`](../user/plugins.md)；整体设计见
> [`../plans/plugin-system-plan.md`](../plans/plugin-system-plan.md) §6.1。

---

## 1. 架构总览

Pointer 是 MCP 客户端，实现位于 `crates/pointer-core/src/plugins/mcp.rs`：

```
McpServerDecl (配置声明)
      │  stdio / http
      ▼
McpClient  ── transport ──► Stdio(子进程管道) | Http(streamable HTTP POST)
      │  request/notify/list_tools/call_tool（JSON-RPC 2.0）
      ▼
McpSessionManager ── 会话表 + 状态表（watchdog 驱动崩溃重启 / degraded）
      ▼
ToolRegistry ── 工具注册 mcp.<server>.<tool>（与插件工具同一审批/allowlist 链路）
```

关键链路：`AppState` 启动/热重载 → `sync_global_mcp_sessions`（全局）或
`activate_mcp_servers`（插件）→ `activate_mcp_servers_with` 按 transport 分派
`connect_stdio` / `connect_http` → 握手 + `tools/list` 注册工具。

## 2. 配置声明（McpServerDecl）

结构：`crates/pointer-core/src/plugins/manifest.rs`

| 字段 | 类型 | 说明 |
|------|------|------|
| `name` | String | server 名（工具命名 `mcp.<name>.<tool>`） |
| `transport` | String | `stdio`（默认）或 `http` |
| `command` | String | stdio：启动命令 |
| `args` | Vec\<String> | stdio：命令参数 |
| `env` | HashMap\<String,String> | stdio：环境变量 |
| `url` | Option\<String> | http：服务地址（必填），端口在 URL 内 |
| `headers` | Option\<HashMap\<String,String>> | http：附加请求头（如 `Authorization`） |

反序列化 `#[serde(default)]` 保证旧配置兼容；序列化时 `url`/`headers` 为 `None`
则跳过，避免空字段落盘。

### 配置载体（全局 MCP）

**主载体：客户端用户配置** `UserSettings.global_mcp_servers`（界面直接读写，
持久化到 `user_settings.json`，桌面/Web 共用）。`AppState::save_global_mcp_servers`
保存列表 + 热重载。

**兼容来源：** `pointer-server.toml` 的 `[[mcp_servers.server]]`（`server_config.rs`
解析 + `PARSED_MCP` 缓存），**仅当用户配置为空时**读取（`GlobalMcpConfig::from_server_config`）。

### 传输协议

- **stdio**：`Command::spawn` 子进程，stdin/stdout 逐行 JSON-RPC（newline-delimited）；
  后台读线程按 `id` 分发响应到 `pending` 表；stdout EOF 置死标记供 watchdog。
- **http（streamable HTTP）**：POST `url`，`Accept: application/json, text/event-stream`；
  响应解析支持 `application/json` 直读 与 SSE `data:` 事件（取含 `id` 的事件）。

> ⚠️ **async 安全**：`reqwest::blocking::Client` 内部持有 tokio runtime，在 tokio
> 异步上下文（axum handler / `#[tokio::test]`）创建会 panic。因此 HTTP 请求在
> **独立线程内创建/释放 blocking client**（`McpClient::request` / `notify` 的 Http 分支）。
> 已知限制：每次调用独立线程 + 独立连接，无连接复用；工具调用频率低时可接受，
> 若远程服务成为主力路径再评估连接池。

## 3. 生命周期

| 环节 | 行为 |
|------|------|
| 全局（界面配置） | `AppState::new` 装配；`save_global_mcp_servers` / `reload_global_mcp` 热重载（全量关旧会话 → 注销工具 → 重新装配） |
| 插件内声明 | `plugin_enable` 时自动 `activate_mcp_servers`；禁用/卸载 → `shutdown_plugin` + 注销 |
| watchdog | `mcp_watchdog_cycle`（2s 周期）：会话不存在或全部进程退出 → 重建（指数退避）；连续失败 ≥ `MAX_MCP_RESTART_FAILURES`(5) → degraded（停止自动重启） |
| 关闭 | `McpSessionManager::shutdown_plugin`（幂等）；`McpClient::drop` → kill 子进程 / 置死标记 |

## 4. 命名与冲突

- 工具命名：`mcp.<server>.<tool>`（与插件裸工具名区分）
- `doc_source`：`plugin:__global__:mcp:<server>:<tool>`（全局）/ `plugin:<id>:mcp:<server>:<tool>`（插件）
- **同名冲突全局优先**：全局 server 先注册；插件启用同名 server 时 `plugin_enable` 拒绝（不静默覆盖）

## 5. 管理 API

### HTTP（web / 本地服务）

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/api/mcp` | 全局 MCP 视图（servers + enabled） |
| PUT | `/api/mcp` | 保存全局 MCP server 列表（body: `Vec<McpServerDecl>`，持久化 + 热重载） |

### Tauri（桌面）

- `list_mcp_servers()` → `GlobalMcpView`
- `save_mcp_servers(servers: Vec<McpServerDecl>)` → `GlobalMcpView`
- `reload_mcp_servers()` / `restart_mcp_server()`（既有）

### 前端

`src/components/settings/panels/McpPanel.vue`（设置 → MCP）：
服务列表 + 状态 + 添加/编辑/删除弹窗（连接方式二选一：远程服务 URL / 本机程序命令）。
API 适配：`src/lib/api.ts` / `tauri.ts` / `web.ts`（`listMcpServers` / `saveMcpServers` / `restartMcpServer`）。

## 6. 测试

`crates/pointer-core/src/plugins/e2e_tests.rs`（`cargo test -p pointer-core --lib plugins::e2e_tests`）：

| 用例 | 覆盖 |
|------|------|
| `global_mcp_basic_flows` | 全局 stdio 装配 + 调用 + 热重载清空 |
| `global_mcp_conflict_rejects_plugin_enable` | 同名冲突全局优先 |
| `global_mcp_saved_via_ui_persists_across_restart` | 界面保存 → 重启 AppState 从 user_settings 恢复 |
| `global_mcp_http_transport_connects_remote_service` | HTTP 远程服务连接 + 调用（本地 Python 最小 server） |
| `global_mcp_crash_recovers_via_watchdog` | 崩溃自动重启 |

> 注意：e2e 需要 `python3`（HTTP 用例的本地测试 server）。

[返回文档总索引](../README.md)
