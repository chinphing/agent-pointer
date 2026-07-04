# 设计文档（design）

本目录存放**设计方案**、路线图、实现计划与技术提案，供评审、估人天及与代码交叉核对。

| 文档 | 说明 |
|------|------|
| [conversation-store-append-migration.md](conversation-store-append-migration.md) | SQLite 对话存储 + **ConversationTranscript**：P0–P2a、append-only、tool 行 canonical |
| [persistent-memory-and-self-improvement.md](persistent-memory-and-self-improvement.md) | 跨会话 **MEMORY/USER** 记忆与 **Self-improvement review**（后台自省）设计稿；参考 Hermes，**暂未实现** |
| [explore-subagent-for-coder.md](explore-subagent-for-coder.md) | 内置 **explore** worker、`run_subagent` 与 Cursor Explore 对齐、Lead→explore 约定 |
| [agent-scope-rules-roadmap.md](agent-scope-rules-roadmap.md) | 分层 scope 规则（P0–P1 已实现：Instruction priority、User Coding Rules；P2–P5 路线图） |
| [coder-agent-capability-roadmap.md](coder-agent-capability-roadmap.md) | Coder 能力增强路线图与优先级矩阵 |
| [computer-use-implementation-plan.md](computer-use-implementation-plan.md) | Computer Use Agent（视觉桌面）迁移与分期计划 |
| [computer-compact-dock-bar.md](computer-compact-dock-bar.md) | 电脑操控时 OS 窗口收缩为右下角 Dock Bar（已确认：OS 级、结束自动展开、含子 agent） |
| [refactor-roadmap.md](refactor-roadmap.md) | 代码重构路线图（文件/函数行数、分步 P0–P3） |
| [trigger-and-event-driven-refactor.md](trigger-and-event-driven-refactor.md) | Trigger & Event-Driven 重构：`RunDispatcher` 统一入口、队列、事件总线、钩子、HTTP Runs API、Webhook、Cron（分阶段交付 + 范围取舍） |
| [file-grep-d-enhancement-proposal.md](file-grep-d-enhancement-proposal.md) | `file:grep` 增强草案（对齐 rg 行为等） |

产品行为与外部集成配置见 **[`../developer/`](../developer/README.md)**；用户使用教程见 **[`../user/`](../user/README.md)**；贡献者打包见 **[`../contributing/`](../contributing/README.md)**。全局索引见 **[`../README.md`](../README.md)**。
