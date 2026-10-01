# Internals

English | [简体中文](../../internals/README.md)

Implementation details and message-pipeline notes, for the **Pointer team maintaining `pointer-core`**.

For user documentation see **[`../user/`](../user/README.md)**; for extension integration see **[`../developer/`](../developer/README.md)**.

| Document | Description |
|------|------|
| [llm-prompt-assembly-order.md](../../internals/llm-prompt-assembly-order.md) | Order in which `messages` and `SystemPromptSections` are assembled before `stream_chat` |
| [long-chat-memory.md](../../internals/long-chat-memory.md) | Long-conversation peak memory: base + injected_tail, wire-before-spawn |
| [context-compression.md](../../internals/context-compression.md) | Context compression: dynamic summary budget, keeping the last 3 turns of user text on failure, reload consistency |
| [trigger-dispatcher.md](../../internals/trigger-dispatcher.md) | Unified trigger entry point `RunDispatcher`: queues, Cron (for Webhook see [`../../developer/webhook-api.md`](../../developer/webhook-api.md)) |
| [standalone-server-deployment.md](standalone-server-deployment.md) | Complete pointer-server standalone deployment flow (build, License, config template, acceptance) |
| [agent-task-board-and-verification.md](../../internals/agent-task-board-and-verification.md) | Task board, the `verification` field and multi-agent conventions |
| [sidebar-conversation-search.md](../../internals/sidebar-conversation-search.md) | Sidebar conversation search: FTS hits + match-centered snippets |
| [sidebar-project-navigation.md](../../internals/sidebar-project-navigation.md) | Sidebar project grouping and entry-point behaviour |
| [taskboard-lifecycle-and-fields.md](../../internals/taskboard-lifecycle-and-fields.md) | Task Board v4 lifecycle and field semantics |
| [terminal-shell-path.md](../../internals/terminal-shell-path.md) | `terminal` tool PATH / shell behaviour per platform |
| [macos-computer-permissions.md](../../internals/macos-computer-permissions.md) | macOS computer-control permissions |
| [user-platform-config-split.md](../../internals/user-platform-config-split.md) | User / platform configuration split; the platform model catalogue is the single source of truth for platform providers and tier defaults |
| [settings-provider-ui.md](../../internals/settings-provider-ui.md) | Settings page Provider UI |
| [task-board-v2-schema.md](../../internals/task-board-v2-schema.md) | Task board v2 schema |
| [task-board-unified-milestone-inject.md](../../internals/task-board-unified-milestone-inject.md) | Task board milestone injection |
| [task-board-parent-child-coordination.md](../../internals/task-board-parent-child-coordination.md) | Task board parent/child coordination |
| [task-board-campaign-work-queue-spec.md](../../internals/task-board-campaign-work-queue-spec.md) | Campaign work queue |

[Back to the documentation index](../README.md)
