# 内部机制（internals）

实现细节与消息管线说明，面向**维护 `pointer-core` 的 Pointer 团队**。

用户使用见 **[`../user/`](../user/README.md)**；扩展集成见 **[`../developer/`](../developer/README.md)**。

| 文档 | 说明 |
|------|------|
| [llm-prompt-assembly-order.md](llm-prompt-assembly-order.md) | `stream_chat` 前 `messages` 与 `SystemPromptSections` 拼接顺序 |
| [long-chat-memory.md](long-chat-memory.md) | 长会话峰值内存：base + injected_tail、wire-before-spawn |
| [context-compression.md](context-compression.md) | 上下文压缩动态摘要预算、失败时保留最近 3 轮用户原文、重载一致性 |
| [trigger-dispatcher.md](trigger-dispatcher.md) | 统一触发入口 `RunDispatcher`：队列、Cron（Webhook 见 [`../developer/webhook-api.md`](../developer/webhook-api.md)） |
| [standalone-server-deployment.md](standalone-server-deployment.md) | pointer-server standalone 完整部署流程（构建、License、配置模板、验收） |
| [agent-task-board-and-verification.md](agent-task-board-and-verification.md) | 任务板、`verification` 字段与多 Agent 约定 |
| [sidebar-conversation-search.md](sidebar-conversation-search.md) | 侧边栏会话搜索：FTS 命中 + match-centered snippet |
| [sidebar-project-navigation.md](sidebar-project-navigation.md) | 侧边栏项目分组与入口行为 |
| [taskboard-lifecycle-and-fields.md](taskboard-lifecycle-and-fields.md) | Task Board v4 生命周期与字段语义 |
| [terminal-shell-path.md](terminal-shell-path.md) | `terminal` 工具在各平台的 PATH / shell 行为 |
| [pointer-build-toml.md](pointer-build-toml.md) | 编译期 `.pointer-build.toml` |
| [macos-computer-permissions.md](macos-computer-permissions.md) | macOS 电脑操控权限 |
| [user-platform-config-split.md](user-platform-config-split.md) | 用户 / 平台配置拆分；平台模型目录是平台服务商与档位默认的唯一来源 |
| [settings-provider-ui.md](settings-provider-ui.md) | 设置页 Provider UI |
| [task-board-v2-schema.md](task-board-v2-schema.md) | 任务板 v2 schema |
| [task-board-unified-milestone-inject.md](task-board-unified-milestone-inject.md) | 任务板 milestone 注入 |
| [task-board-parent-child-coordination.md](task-board-parent-child-coordination.md) | 任务板父子协调 |
| [task-board-campaign-work-queue-spec.md](task-board-campaign-work-queue-spec.md) | Campaign 工作队列 |

[返回文档总索引](../README.md)
