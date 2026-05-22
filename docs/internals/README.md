# 内部机制（internals）

实现细节与消息管线说明，面向维护 `pointer-core` 的开发者。

| 文档 | 说明 |
|------|------|
| [llm-prompt-assembly-order.md](llm-prompt-assembly-order.md) | `stream_chat` 前 `messages` 与 `SystemPromptSections`（cacheable / dynamic）拼接顺序 |
| [agent-extension-hooks.md](agent-extension-hooks.md) | 扩展注册表与钩子触发点 |
| [agent-task-board-and-verification.md](agent-task-board-and-verification.md) | 任务板、`verification` 字段与多 Agent 约定 |
| [desktop-oauth-web-integration.md](desktop-oauth-web-integration.md) | 桌面 OAuth 回调后首页 `?desktop_oauth=success` 与官网一次性提示 |

[返回文档总索引](../README.md)
