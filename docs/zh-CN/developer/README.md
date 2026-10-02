# 开发者文档
[English](../../en/developer/README.md) | 简体中文

面向**外部开发者、集成方与 Skill 作者**：对接 IM、编写 Skill、扩展 Agent、自部署。

**终端用户使用教程**见 **[`../user/`](../user/README.md)**。

先读 [architecture.md](architecture.md)，再按 [DEVELOPMENT.md](../../../DEVELOPMENT.md) 跑起来。

> 本页按侧栏「开发」分区的四组组织：**概念 / 工具与协议 / 集成 / 排障**（外加「其他」）。`standalone-deployment.md` 在侧栏「部署」分区。

## 打包与部署：四格入口

**打包口味**（`managed` 集中管理 / `standalone` 独立）× **运行形态**（客户端 / 服务端）四格，每格都有**构建命令 / 需要的变量 / 产物位置 / 怎么验证 / 外部依赖**，入口见 [`../zh-CN/deploy/README.md`](../deploy/README.md)（详细手册 [`editions.md`](../deploy/editions.md)）：

| 口味 | 客户端（Tauri 桌面 App） | 服务端（`pointer-server`） |
|------|--------------------------|-----------------------------|
| **`managed`**（集中管理，构建期注入控制面域名） | [构建与部署](../deploy/README.md#managed-client) | [构建与部署](../deploy/README.md#managed-server) |
| **standalone**（独立，默认：不设口味） | [构建与部署](../deploy/README.md#standalone-client) | [构建与部署](../deploy/README.md#standalone-server)<br>完整交付流程见 [../internals/standalone-server-deployment.md](../internals/standalone-server-deployment.md) |

`official` 一词仅指 Pointer 官方发布，不是 `POINTER_EDITION` 的取值。跨平台环境准备见 [../contributing/cross-platform-build.md](../contributing/cross-platform-build.md)。

## 概念

| 文档 | 说明 |
|------|------|
| [architecture.md](architecture.md) | 桌面 / Web 共用 pointer-core；crate 地图与架构全景图 |
| [workspace-root.md](workspace-root.md) | 会话工作区根路径解析与沙箱目录布局 |
| [attachment-storage.md](attachment-storage.md) | 附件落盘位置与命名（`session-sandboxes/`） |
| [agent-extension-hooks.md](agent-extension-hooks.md) | 扩展注册表与钩子触发点 |
| [skills-compatibility.md](skills-compatibility.md) | `SKILL.md` 格式与 Codex / Agent 目录兼容 |
| [skills-persistence.md](skills-persistence.md) | 加载顺序、持久化、Curator（实现向） |

用户导入与启用见 [`../user/skills.md`](../user/skills.md)。

## 工具与协议

| 文档 | 说明 |
|------|------|
| [native-tool-calling-protocol.md](native-tool-calling-protocol.md) | Provider native tool calling 约定 |
| [pointer-run-subagent.md](pointer-run-subagent.md) | `run_subagent`、`allowAgents`、内置 explore、后台 `background` / `terminal.blockUntilMs` / `job` |
| [file-tool-write-scope.md](file-tool-write-scope.md) | `file_write` / `file_edit` 允许的写入目录 |
| [file-tool-output-limits.md](file-tool-output-limits.md) | 读文件 / glob / 列举 / 搜索 / 终端回包上限与终端超时（设置 → 内容上限 / 终端超时；`file_read` 行数、`file_glob` / `file_list` 条数为实现常量） |
| [web-search-tool.md](web-search-tool.md) | `web_search` 工具行为与 DashScope API |
| [web-fetch-tool.md](web-fetch-tool.md) | `web_fetch` 抓取公开 URL（Hermes `web_extract` 对齐） |
| [terminal-environment-variables.md](terminal-environment-variables.md) | **`terminal`** 子进程环境变量（`WORKING_DIR`、`SESSION_USER_ID`、`DATA_DIR`、`SKILL_DIR`） |
| [terminal-interactive-input.md](terminal-interactive-input.md) | SSH / sudo 等交互输入：应用内密码弹窗、ASKPASS、提示词约定 |
| [session-search-output-limits.md](session-search-output-limits.md) | `session_search` / `session_read` 命中截断、按工具名剔除旧回包、工具 `matches[]` 上限；侧栏展开全部命中 |
| [logging.md](logging.md) | `run_chat` 相关 info / debug 选用约定 |

用户设置项见 [`../user/subagents.md`](../user/subagents.md)；工作区 lint 配置见 [`../user/project-lint.md`](../user/project-lint.md)。

## 集成

| 文档 | 说明 |
|------|------|
| [channel-integration.md](channel-integration.md) | IM 通道完整对接（长连接 / Webhook、各平台步骤与排查） |
| [feishu-cli-integration-sop.md](feishu-cli-integration-sop.md) | 飞书 CLI 集成 SOP |
| [lark-cli-quickstart.md](lark-cli-quickstart.md) | Lark CLI 快速上手 |
| [cloud-host-integration.md](cloud-host-integration.md) | 云实例自部署、环境变量、认证链路 |
| [desktop-oauth-web-integration.md](desktop-oauth-web-integration.md) | 桌面 OAuth 回调与官网 `?desktop_oauth=success` |
| [mcp.md](mcp.md) | MCP 客户端接入：stdio / HTTP 双传输、配置载体、生命周期、管理 API |
| [webhook-api.md](webhook-api.md) | 通用 Webhook API（触发 Agent、鉴权、附件、同步/异步） |

用户侧 IM 操作摘要见 [`../user/im-channels.md`](../user/im-channels.md)。

## 排障

| 文档 | 说明 |
|------|------|
| [chat-run-errors.md](chat-run-errors.md) | `StreamEvent::Error` 仅由 `run_chat` 统一发出 |
| [chat-stream-resync.md](chat-stream-resync.md) | Web SSE 弱网丢事件：resync / 对账执行态与消息 |
| [platform-auth-refresh-errors.md](platform-auth-refresh-errors.md) | 登录刷新：网络抖动 vs 需要重新登录 |
| [rust-text-truncation.md](rust-text-truncation.md) | UTF-8 安全字符串截断（`text_util`） |
| [vite-chunking.md](vite-chunking.md) | 前端 Vite `manualChunks` 与按需加载（Chart / Workspace） |
| [session-user-id.md](session-user-id.md) | 会话 `session_user_id` 持久化与解析 |
| [turn-file-baseline-review.md](turn-file-baseline-review.md) | 轮次页脚修改摘要、文件基线与右侧栏 Review |
| [conversation-message-position-collision.md](conversation-message-position-collision.md) | 会话消息 `position` 碰撞（上下文压缩），已修复记录 |

## 其他

| 文档 | 说明 |
|------|------|
| [cli.md](cli.md) | `pointer-server` 参数、npm scripts、部署脚本与哪些包有 bin |
| [standalone-local-login.md](standalone-local-login.md) | Standalone 账号密码 + 第三方 `?sso=` 本地验签登录 |
| [standalone-deployment.md](standalone-deployment.md) | pointer-server 配置参考：本地认证与 SSO、License（仅 managed 口味强制）、访问控制、`[server]` 品牌参数与 CORS、配置文件发现顺序（侧栏里在「部署」分区） |

## 从源码构建 Pointer

见 [`../contributing/cross-platform-build.md`](../contributing/cross-platform-build.md)。

[返回文档总索引](../README.md)
