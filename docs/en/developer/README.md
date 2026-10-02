# Developer documentation
English | [简体中文](../../zh-CN/developer/README.md)

For **external developers, integrators and Skill authors**: connecting IM, writing Skills, extending agents, self-hosting.

**End-user tutorials** are in **[`../user/`](../user/README.md)**.

Read [architecture.md](architecture.md) first, then get it running with [DEVELOPMENT.md](../DEVELOPMENT.md).

> This page follows the four groups of the 开发 (Development) sidebar section: **Concepts / Tools and protocols / Integration / Troubleshooting** (plus Other). `standalone-deployment.md` lives under Deployment.

## Packaging and deployment: the four-cell entry

**Packaging flavour** (`managed` centrally managed / `standalone` independent) × **runtime form** (client / server) makes four cells; each cell has its **build command / required variables / artifact location / how to verify / external dependencies**. Entry point: [`../deploy/README.md`](../deploy/README.md) (detailed manual [`editions.md`](../deploy/editions.md)):

| Flavour | Client (Tauri desktop app) | Server (`pointer-server`) |
|------|--------------------------|-----------------------------|
| **`managed`** (centrally managed; control-plane domains injected at build time) | [Build and deploy](../deploy/README.md#managed-client) | [Build and deploy](../deploy/README.md#managed-server) |
| **standalone** (independent, default: no flavour set) | [Build and deploy](../deploy/README.md#standalone-client) | [Build and deploy](../deploy/README.md#standalone-server)<br>The full delivery flow is in [../internals/standalone-server-deployment.md](../internals/standalone-server-deployment.md) |

The word `official` refers only to an official Pointer release; it is not a value of `POINTER_EDITION`. For cross-platform environment setup see [../contributing/cross-platform-build.md](../contributing/cross-platform-build.md).

## Concepts

| Document | Description |
|------|------|
| [architecture.md](architecture.md) | Desktop / web sharing pointer-core; crate map and architecture overview |
| [workspace-root.md](workspace-root.md) | Session workspace root path resolution and sandbox directory layout |
| [attachment-storage.md](attachment-storage.md) | Attachment storage location and naming (`session-sandboxes/`) |
| [agent-extension-hooks.md](agent-extension-hooks.md) | Extension registry and hook trigger points |
| [skills-compatibility.md](skills-compatibility.md) | `SKILL.md` format and Codex / Agent directory compatibility |
| [skills-persistence.md](skills-persistence.md) | Load order, persistence, Curator (implementation-oriented) |

For user import and enabling see [`../user/skills.md`](../user/skills.md).

## Tools and protocols

| Document | Description |
|------|------|
| [native-tool-calling-protocol.md](native-tool-calling-protocol.md) | Provider native tool calling conventions |
| [pointer-run-subagent.md](pointer-run-subagent.md) | `run_subagent`, `allowAgents`, built-in explore, background `background` / `terminal.blockUntilMs` / `job` |
| [file-tool-write-scope.md](file-tool-write-scope.md) | Directories `file_write` / `file_edit` are allowed to write to |
| [file-tool-output-limits.md](file-tool-output-limits.md) | Read file / glob / list / search / terminal response caps and terminal timeouts (Settings → Content limits / Terminal timeouts) |
| [web-search-tool.md](web-search-tool.md) | `web_search` tool behaviour and the DashScope API |
| [web-fetch-tool.md](web-fetch-tool.md) | `web_fetch` fetching public URLs (aligned with Hermes `web_extract`) |
| [terminal-environment-variables.md](terminal-environment-variables.md) | **`terminal`** subprocess environment variables (`WORKING_DIR`, `SESSION_USER_ID`, `DATA_DIR`, `SKILL_DIR`) |
| [terminal-interactive-input.md](terminal-interactive-input.md) | Interactive input for SSH / sudo etc.: in-app password modal, ASKPASS, prompt conventions |
| [session-search-output-limits.md](session-search-output-limits.md) | `session_search` / `session_read` hit truncation, dropping old responses by tool name, tool `matches[]` caps; the sidebar expands all hits |
| [logging.md](logging.md) | Conventions for choosing info / debug in `run_chat` |

For user settings see [`../user/subagents.md`](../user/subagents.md); workspace lint configuration is in [`../user/project-lint.md`](../user/project-lint.md).

## Integration

| Document | Description |
|------|------|
| [channel-integration.md](channel-integration.md) | Full IM channel integration (long connection / Webhook, per-platform steps and troubleshooting) |
| [cloud-host-integration.md](cloud-host-integration.md) | Self-hosting cloud instances, environment variables, auth chain |
| [desktop-oauth-web-integration.md](desktop-oauth-web-integration.md) | Desktop OAuth callback and the official site's `?desktop_oauth=success` |
| [mcp.md](mcp.md) | MCP client integration: stdio / HTTP dual transport, config carriers, lifecycle, management API |
| [webhook-api.md](webhook-api.md) | Generic Webhook API (triggering agents, auth, attachments, sync/async) |

For user-side IM summaries see [`../user/im-channels.md`](../user/im-channels.md).

## Troubleshooting

| Document | Description |
|------|------|
| [chat-run-errors.md](chat-run-errors.md) | `StreamEvent::Error` is emitted only by `run_chat` |
| [chat-stream-resync.md](chat-stream-resync.md) | Web SSE dropped events on weak networks: resync / reconciling execution state and messages |
| [platform-auth-refresh-errors.md](platform-auth-refresh-errors.md) | Login refresh: network blips vs. needing to sign in again |
| [rust-text-truncation.md](rust-text-truncation.md) | UTF-8 safe string truncation (`text_util`) |
| [vite-chunking.md](vite-chunking.md) | Frontend Vite `manualChunks` and on-demand loading (Chart / Workspace) |
| [session-user-id.md](session-user-id.md) | Conversation `session_user_id` persistence and resolution |
| [turn-file-baseline-review.md](turn-file-baseline-review.md) | Turn footer change summary, file baseline and the right-hand Review panel |
| [conversation-message-position-collision.md](conversation-message-position-collision.md) | Conversation message `position` collision (context compression), fixed |

## Other

| Document | Description |
|------|------|
| [cli.md](cli.md) | `pointer-server` arguments, npm scripts, deployment scripts and which packages have a bin |
| [standalone-local-login.md](standalone-local-login.md) | Standalone username/password + third-party `?sso=` local signature-verified login |
| [standalone-deployment.md](standalone-deployment.md) | pointer-server configuration reference: local auth and SSO, License (enforced only for managed builds), access control, `[server]` branding parameters and CORS, config discovery order (listed under Deployment in the sidebar) |

## Building Pointer from source

See [`../contributing/cross-platform-build.md`](../contributing/cross-platform-build.md).

[Back to the documentation index](../README.md)
