# 开发者文档（developer）

面向**外部开发者、集成方与 Skill 作者**：对接 IM、编写 Skill、扩展 Agent、自部署云实例等。

**终端用户使用教程**见 **[`../user/`](../user/README.md)**。产品内部实现见 [`../internals/`](../internals/README.md)、[`../design/`](../design/README.md)。

## 集成与部署

| 文档 | 说明 |
|------|------|
| [standalone-deployment.md](standalone-deployment.md) | pointer-server 独立部署、Ed25519 License 系统、机器绑定、本地认证 |
| [standalone-local-login.md](standalone-local-login.md) | Standalone 账号密码 + 第三方 `?sso=` 本地验签登录 |
| [channel-integration.md](channel-integration.md) | IM 通道完整对接（长连接 / Webhook、各平台步骤与排查） |
| [webhook-api.md](webhook-api.md) | 通用 Webhook API（触发 Agent、鉴权、附件、同步/异步） |
| [cloud-host-integration.md](cloud-host-integration.md) | 云实例自部署、环境变量、认证链路 |
| [desktop-oauth-web-integration.md](desktop-oauth-web-integration.md) | 桌面 OAuth 回调与官网 `?desktop_oauth=success` |
| [platform-auth-refresh-errors.md](platform-auth-refresh-errors.md) | 登录刷新：网络抖动 vs 需要重新登录 |
| [feishu-cli-integration-sop.md](feishu-cli-integration-sop.md) | 飞书 CLI 集成 SOP |
| [lark-cli-quickstart.md](lark-cli-quickstart.md) | Lark CLI 快速上手 |

用户侧 IM / 云主机操作摘要见 [`../user/im-channels.md`](../user/im-channels.md)、[`../user/cloud-host.md`](../user/cloud-host.md)。

## 技能（Skills）

| 文档 | 说明 |
|------|------|
| [skills-compatibility.md](skills-compatibility.md) | `SKILL.md` 格式与 Codex / Agent 目录兼容 |
| [skills-persistence.md](skills-persistence.md) | 加载顺序、持久化、Curator（实现向） |

用户导入与启用见 [`../user/skills.md`](../user/skills.md)。

## Agent 与子代理

| 文档 | 说明 |
|------|------|
| [pointer-run-subagent.md](pointer-run-subagent.md) | `run_subagent`、`allowAgents`、内置 explore |
| [agent-extension-hooks.md](agent-extension-hooks.md) | 扩展注册表与钩子触发点 |

用户设置项见 [`../user/subagents.md`](../user/subagents.md)。

## 工具与运行时

| 文档 | 说明 |
|------|------|
| [file-tool-write-scope.md](file-tool-write-scope.md) | `file_write` / `file_edit` 允许的写入目录 |
| [web-search-tool.md](web-search-tool.md) | `web_search` 工具行为与 DashScope API |
| [web-fetch-tool.md](web-fetch-tool.md) | `web_fetch` 抓取公开 URL（Hermes `web_extract` 对齐） |
| [terminal-environment-variables.md](terminal-environment-variables.md) | **`terminal`** 子进程环境变量（`WORKING_DIR`、`SESSION_USER_ID`） |
| [workspace-root.md](workspace-root.md) | 会话工作区根路径解析与沙箱目录布局 |
| [session-user-id.md](session-user-id.md) | 会话 `session_user_id` 持久化与解析 |
| [rust-text-truncation.md](rust-text-truncation.md) | UTF-8 安全字符串截断（`text_util`） |
| [logging.md](logging.md) | `run_chat` 相关 info / debug 选用约定 |
| [chat-run-errors.md](chat-run-errors.md) | `StreamEvent::Error` 仅由 `run_chat` 统一发出 |

工作区 lint 配置（用户向）见 [`../user/project-lint.md`](../user/project-lint.md)。

## 协议

| 文档 | 说明 |
|------|------|
| [native-tool-calling-protocol.md](native-tool-calling-protocol.md) | Provider native tool calling 约定 |

## 从源码构建 Pointer

见 [`../contributing/cross-platform-build.md`](../contributing/cross-platform-build.md)。

[返回文档总索引](../README.md)
