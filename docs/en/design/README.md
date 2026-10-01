# Design documents (design)

English | [简体中文](../../design/README.md)

This directory holds **design proposals**, roadmaps, implementation plans and technical proposals, for review, effort estimation and cross-checking against the code.

| Document | Description |
|------|------|
| [pointer-architecture.zh-CN.svg](../../design/pointer-architecture.zh-CN.svg) | Architecture overview (Chinese, referenced by README.zh-CN) |
| [pointer-architecture.en.svg](../../design/pointer-architecture.en.svg) | Architecture overview (English README) |
| [pointer-architecture.svg](../../design/pointer-architecture.svg) | Default architecture diagram, kept in sync with the Chinese version |
| [conversation-store-append-migration.md](../../design/conversation-store-append-migration.md) | SQLite conversation storage + **ConversationTranscript**: P0–P2a, append-only, canonical tool rows |
| [persistent-memory-and-self-improvement.md](../../design/persistent-memory-and-self-improvement.md) | Cross-conversation **MEMORY/USER** memory and **Self-improvement review** (background reflection) design draft; modelled on Hermes, **not implemented yet** |
| [explore-subagent-for-coder.md](../../design/explore-subagent-for-coder.md) | Built-in **explore** worker, `run_subagent`, alignment with Cursor Explore, Lead→explore conventions |
| [conversation-scoped-messages-v2.md](../../design/conversation-scoped-messages-v2.md) | Frontend conversation **anchor / scoped dual containers**, `SpawnId` replacing the traceId primary key, **ConversationScopedStore** replacing ScopedTraceIndex; includes a phased implementation plan |
| [async-subagent-and-terminal.md](../../design/async-subagent-and-terminal.md) | Background `run_subagent` / `terminal`: Foreground/Background two-tier waiting; **one per-conversation worker queue** (a foreground borrowed slot does not enter the job table); does not occupy a session lane |
| [session-search-scope-extension.md](../../design/session-search-scope-extension.md) | Conversation search **`session_search` + `session_read`** (P0–P1 landed: the instance column) |
| [execution-sandbox-candidates.md](../../design/execution-sandbox-candidates.md) | Execution sandbox candidates (**pending decision**): lightweight local isolation vs. CubeSandbox / E2B strong isolation on a Linux server |
| [execution-sandbox-discussion.md](../../design/execution-sandbox-discussion.md) | Execution sandbox discussion minutes: conclusions, local candidates, landstrip deep read, command boundaries and effort estimates |
| [agent-scope-rules-roadmap.md](../../design/agent-scope-rules-roadmap.md) | Layered scope rules (P0–P1 implemented: Instruction priority, User Coding Rules; P2–P5 roadmap) |
| [coder-agent-capability-roadmap.md](../../design/coder-agent-capability-roadmap.md) | Coder capability roadmap and priority matrix |
| [computer-use-implementation-plan.md](../../design/computer-use-implementation-plan.md) | Computer Use Agent (visual desktop) migration and phased plan |
| [computer-compact-dock-bar.md](../../design/computer-compact-dock-bar.md) | Collapsing the OS window into a bottom-right Dock Bar during computer control (confirmed: OS level, auto-expands on finish, includes sub-agents) |
| [mobile-reimburse-assistant-mockup.html](../../design/mobile-reimburse-assistant-mockup.html) | Sample assistant mobile three-state interaction mockup (welcome tip / filling in / filled) |
| [web-branding-welcome-elapsed.md](../../design/web-branding-welcome-elapsed.md) | Web branding: welcome tip + turn elapsed prefix (server-customisable, unchanged by default) |
| [trigger-and-event-driven-refactor.md](../../design/trigger-and-event-driven-refactor.md) | Trigger & Event-Driven refactor: `RunDispatcher` unified entry, queues, event bus, hooks, HTTP Runs API, Webhook, Cron (phased delivery + scope trade-offs) |
| [file-grep-d-enhancement-proposal.md](../../design/file-grep-d-enhancement-proposal.md) | `file:grep` enhancement draft (aligning with rg behaviour, etc.) |
| [control-plane-and-editions.md](control-plane-and-editions.md) | **Account and control-plane rework**: three usage modes, client/server packaging, gating, P0–P3 |

For product behaviour and external integration configuration see **[`../developer/`](../developer/README.md)**; for user tutorials see **[`../user/`](../user/README.md)**; for contributor packaging see **[`../contributing/`](../contributing/README.md)**. For the global index see **[`../README.md`](../README.md)**.
