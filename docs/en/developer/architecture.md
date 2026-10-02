# Architecture

English | [简体中文](../../zh-CN/developer/architecture.md)

The desktop and web clients share one implementation of conversations and tools.

```text
Vue UI
├─ Desktop: Tauri Adapter → src-tauri → crates/pointer-core
└─ Web: Web Adapter → server (HTTP/SSE) → crates/pointer-core
```

## Crate map

Workspace members are declared in `[workspace].members` in the root `Cargo.toml`; there are 5:

| Crate / directory | Type | Artifact | Responsibility |
| --- | --- | --- | --- |
| `crates/pointer-core` | **lib (no bin)** | — | All the business core: conversation engine, tool execution, LLM providers, Skills / plugins / MCP, Agents, scheduling, storage, License |
| `crates/pointer-channels` | **lib (no bin)** | — | IM channel adapters (Feishu / DingTalk / WeCom / WeChat) + pairing + outbound delivery |
| `server/` | bin | `pointer-server` | axum HTTP/SSE + same-origin Vue `dist`; the web client and self-hosted deployments |
| `src-tauri/` | bin | `pointer-app` (desktop artifact name `Pointer`) | Tauri 2 desktop shell: IPC, tray, updater, cloud host WebView, macOS permissions |
| `tools/license-gen/` | bin | `license-gen` | License issuance tool |

> ⚠️ **`pointer-core` and `pointer-channels` are pure libs** (only `src/lib.rs`, no `src/main.rs`). `cargo run -p pointer-core` does not work — they can only be linked by `pointer-server`, `pointer-app` or tests. To actually run something use `npm run server:dev` / `npm run tauri:dev`; for the command list see [cli.md](cli.md).

### Non-Rust directories

| Path | Responsibility |
| --- | --- |
| `src/` | Vue UI, shared by desktop and web |
| `skills/` | System Skills shipped with the installer |
| `docs/` | Documentation source; the site project lives in `docs-site/` ([docs site rules](../contributing/docs-site.md)) |
| `scripts/` | Build, packaging, version-sync and documentation-check scripts |

One conversation: the UI sends a message → the adapter layer enters `pointer-core` orchestration → the provider streams back → tools / sub-agents execute via the registry → the result is written back to the conversation store and pushed to the UI.

## Architecture overview

![Pointer architecture: the desktop app, the web app and IM channels share one pointer-core — the UI handles interaction, while orchestration, tools, Skills and sessions live in the core](/pointer-architecture.en.svg)

[中文版](/pointer-architecture.zh-CN.svg) · Source: [`design/pointer-architecture.en.svg`](../../zh-CN/design/pointer-architecture.en.svg)

For implementation details see [../internals/](../internals/README.md). For the default cloud address of the two builds see [../deploy/editions.md](../deploy/editions.md).
