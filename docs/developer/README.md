# 开发者文档（developer）

面向**外部开发者、集成方与 Skill 作者**：对接 IM、编写 Skill、扩展 Agent、自部署云实例等。

**终端用户使用教程**见 **[`../user/`](../user/README.md)**。产品内部实现见 [`../internals/`](../internals/README.md)、[`../design/`](../design/README.md)。

## 集成与部署

| 文档 | 说明 |
|------|------|
| [channel-integration.md](channel-integration.md) | IM 通道完整对接（长连接 / Webhook、各平台步骤与排查） |
| [cloud-host-integration.md](cloud-host-integration.md) | 云实例自部署、环境变量、认证链路 |
| [desktop-oauth-web-integration.md](desktop-oauth-web-integration.md) | 桌面 OAuth 回调与官网 `?desktop_oauth=success` |
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
| [terminal-environment-variables.md](terminal-environment-variables.md) | **`terminal`** 子进程环境变量（`WORKING_DIR`、`SESSION_USER_ID`） |
| [workspace-root.md](workspace-root.md) | 会话工作区根路径解析与沙箱目录布局 |
| [session-user-id.md](session-user-id.md) | 会话 `session_user_id` 持久化与解析 |

工作区 lint 配置（用户向）见 [`../user/project-lint.md`](../user/project-lint.md)。

## 协议

| 文档 | 说明 |
|------|------|
| [native-tool-calling-protocol.md](native-tool-calling-protocol.md) | Provider native tool calling 约定 |

## 从源码构建 Pointer

见 [`../contributing/cross-platform-build.md`](../contributing/cross-platform-build.md)。

[返回文档总索引](../README.md)
