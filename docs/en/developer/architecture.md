# Architecture

English | [简体中文](../../developer/architecture.md)

The desktop and web clients share one implementation of conversations and tools.

```text
Vue UI
├─ Desktop: Tauri Adapter → src-tauri → crates/pointer-core
└─ Web: Web Adapter → server (HTTP/SSE) → crates/pointer-core
```

## Directories

| Path | Responsibility |
| --- | --- |
| `src/` | Vue UI, shared by desktop and web |
| `src-tauri/` | Desktop shell, IPC, updater plugin |
| `server/` | HTTP/SSE for web and standalone deployments |
| `crates/pointer-core` | Models, tools, Skills, conversations, storage |
| `crates/pointer-channels` | IM channels |
| `skills/` | System Skills shipped with the installer |

One conversation: the UI sends a message → the adapter layer enters `pointer-core` orchestration → the provider streams back → tools / sub-agents execute via the registry → the result is written back to the conversation store and pushed to the UI.

Architecture overview: [Chinese](../../design/pointer-architecture.zh-CN.svg) · [English](../../design/pointer-architecture.en.svg).

For implementation details see [../internals/](../internals/README.md). For the default cloud address of the two builds see [../deploy/editions.md](../deploy/editions.md).
