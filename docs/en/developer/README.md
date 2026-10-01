# Developer documentation (developer)

English | [简体中文](../../developer/README.md)

For **external developers, integrators and Skill authors**: connecting IM, writing Skills, extending agents, self-hosting.

**End-user tutorials** are in **[`../user/`](../user/README.md)**.

Read [architecture.md](architecture.md) first, then get it running with [DEVELOPMENT.md](../DEVELOPMENT.md).

## Packaging and deployment: the four-cell entry

**Packaging flavour** (`managed` centrally managed / `standalone` independent) × **runtime form** (client / server) makes four cells; each cell has its **build command / required variables / artifact location / how to verify / external dependencies**. Entry point: [`../deploy/README.md`](../deploy/README.md) (detailed manual [`editions.md`](../deploy/editions.md)):

| Flavour | Client (Tauri desktop app) | Server (`pointer-server`) |
|------|--------------------------|-----------------------------|
| **`managed`** (centrally managed; control-plane domains injected at build time) | [Build and deploy](../deploy/README.md#managed-client) | [Build and deploy](../deploy/README.md#managed-server) |
| **standalone** (independent, default: no flavour set) | [Build and deploy](../deploy/README.md#standalone-client) | [Build and deploy](../deploy/README.md#standalone-server)<br>The full delivery flow is in [../internals/standalone-server-deployment.md](../internals/standalone-server-deployment.md) |

The word `official` refers only to an official Pointer release; it is not a value of `POINTER_EDITION`. For cross-platform environment setup see [../contributing/cross-platform-build.md](../contributing/cross-platform-build.md).

## Integration and deployment

| Document | Description |
|------|------|
| [architecture.md](architecture.md) | Desktop / web sharing pointer-core |
| [standalone-deployment.md](standalone-deployment.md) | pointer-server configuration reference: local auth, License (enforced only for official builds), `[server]` branding parameters and CORS |
| [standalone-local-login.md](../../developer/standalone-local-login.md) | Standalone username/password + third-party `?sso=` local signature-verified login |
| [channel-integration.md](../../developer/channel-integration.md) | Full IM channel integration (long connection / Webhook, per-platform steps and troubleshooting) |
| [webhook-api.md](../../developer/webhook-api.md) | Generic Webhook API (triggering agents, auth, attachments, sync/async) |
| [cloud-host-integration.md](../../developer/cloud-host-integration.md) | Self-hosting cloud instances, environment variables, auth chain |
| [desktop-oauth-web-integration.md](../../developer/desktop-oauth-web-integration.md) | Desktop OAuth callback and the official site's `?desktop_oauth=success` |
| [platform-auth-refresh-errors.md](../../developer/platform-auth-refresh-errors.md) | Login refresh: network blips vs. needing to sign in again |
| [chat-stream-resync.md](../../developer/chat-stream-resync.md) | Web SSE dropped events on weak networks: resync / reconciling execution state and messages |
| [feishu-cli-integration-sop.md](../../developer/feishu-cli-integration-sop.md) | Feishu CLI integration SOP |
| [lark-cli-quickstart.md](../../developer/lark-cli-quickstart.md) | Lark CLI quickstart |

For user-side IM / cloud host summaries see [`../user/im-channels.md`](../user/im-channels.md), [`../user/cloud-host.md`](../user/cloud-host.md).

## Skills

| Document | Description |
|------|------|
| [skills-compatibility.md](../../developer/skills-compatibility.md) | `SKILL.md` format and Codex / Agent directory compatibility |
| [skills-persistence.md](../../developer/skills-persistence.md) | Load order, persistence, Curator (implementation-oriented) |

For user import and enabling see [`../user/skills.md`](../user/skills.md).

## Agents and sub-agents

| Document | Description |
|------|------|
| [pointer-run-subagent.md](../../developer/pointer-run-subagent.md) | `run_subagent`, `allowAgents`, built-in explore, background `background` / `terminal.blockUntilMs` / `job` |
| [agent-extension-hooks.md](../../developer/agent-extension-hooks.md) | Extension registry and hook trigger points |

For user settings see [`../user/subagents.md`](../user/subagents.md).

## Tools and runtime

| Document | Description |
|------|------|
| [file-tool-write-scope.md](../../developer/file-tool-write-scope.md) | Directories `file_write` / `file_edit` are allowed to write to |
| [file-tool-output-limits.md](../../developer/file-tool-output-limits.md) | Read file / glob / list / search / terminal response caps and terminal timeouts (Settings → Content limits / Terminal timeouts; `file_read` line counts and `file_glob` / `file_list` entry counts are implementation constants) |
| [session-search-output-limits.md](../../developer/session-search-output-limits.md) | `session_search` / `session_read` hit truncation, dropping old responses by tool name, tool `matches[]` caps; the sidebar expands all hits |
| [turn-file-baseline-review.md](../../developer/turn-file-baseline-review.md) | Turn footer change summary, file baseline and the right-hand Review panel |
| [web-search-tool.md](../../developer/web-search-tool.md) | `web_search` tool behaviour and the DashScope API |
| [web-fetch-tool.md](../../developer/web-fetch-tool.md) | `web_fetch` fetching public URLs (aligned with Hermes `web_extract`) |
| [terminal-environment-variables.md](../../developer/terminal-environment-variables.md) | **`terminal`** subprocess environment variables (`WORKING_DIR`, `SESSION_USER_ID`, `DATA_DIR`, `SKILL_DIR`) |
| [terminal-interactive-input.md](../../developer/terminal-interactive-input.md) | Interactive input for SSH / sudo etc.: in-app password modal, ASKPASS, prompt conventions |
| [mcp.md](../../developer/mcp.md) | MCP client integration: stdio / HTTP dual transport, config carriers, lifecycle, management API |
| [workspace-root.md](../../developer/workspace-root.md) | Session workspace root path resolution and sandbox directory layout |
| [session-user-id.md](../../developer/session-user-id.md) | Conversation `session_user_id` persistence and resolution |
| [rust-text-truncation.md](../../developer/rust-text-truncation.md) | UTF-8 safe string truncation (`text_util`) |
| [logging.md](../../developer/logging.md) | Conventions for choosing info / debug in `run_chat` |
| [chat-run-errors.md](../../developer/chat-run-errors.md) | `StreamEvent::Error` is emitted only by `run_chat` |
| [vite-chunking.md](../../developer/vite-chunking.md) | Frontend Vite `manualChunks` and on-demand loading (Chart / Workspace) |

For workspace lint configuration (user-facing) see [`../user/project-lint.md`](../../user/project-lint.md).

## Protocols

| Document | Description |
|------|------|
| [native-tool-calling-protocol.md](../../developer/native-tool-calling-protocol.md) | Provider native tool calling conventions |

## Building Pointer from source

See [`../contributing/cross-platform-build.md`](../contributing/cross-platform-build.md).

[Back to the documentation index](../README.md)
