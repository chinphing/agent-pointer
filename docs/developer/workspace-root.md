# Session workspace root

The **workspace root** is the directory used by `file`, `terminal`, `read_lints`, and related tools for a conversation. It is persisted as `workspaceRoot` on the conversation row and exposed to scripts as `WORKING_DIR` (see [terminal-environment-variables.md](terminal-environment-variables.md)). App-local storage (DB, sandboxes, system skills) is exposed separately as `DATA_DIR`; the user skill library (`~/.pointer/skills`) as `SKILL_DIR`.

## Resolution order (`run_chat`)

When a chat run starts, the backend resolves the effective workspace in this order:

1. **User cleared workspace** — Composer ✕ / `workspaceInheritDisabled` → default session sandbox (see below). Does not inherit another conversation's folder.
2. **Non-empty payload** — Client sends an absolute existing directory → use it (user project folder).
3. **Persisted user pick** — `workspace_user_set` is true and stored path is a non-sandbox directory on disk → use it.
4. **Persisted sandbox path** — Stored path is under `{app_data}/session-sandboxes/` and not a user pick → canonical default sandbox for this run (migrates off legacy per-conversation paths).
5. **Default sandbox** — Create or reuse:
   - `{app_data}/session-sandboxes/{sanitize(session_user_id)}/` when `session_user_id` is non-empty
   - `{app_data}/session-sandboxes/_anonymous/{sanitize(conversation_id)}/` when it is empty

**No cross-conversation inheritance.** The old `latest_other_workspace_root` behavior is removed.

Implementation: `chat_service/session_inner.rs` (`resolve_run_workspace`, `ensure_workspace_at_run_start`).

## Desktop conversation ids

Frontend `newConversation` / message client ids use **UUID v4** via `randomUuid()` in
`src/lib/randomUuid.ts` (chat helpers call it as `uid()`). Prefer `crypto.randomUUID()`;
fall back to `crypto.getRandomValues` (and last-resort Math.random) when the browser /
WebView lacks `randomUUID` — otherwise boot and send crash with
`TypeError: crypto.randomUUID is not a function`. Media dirs under `conversation-media/`
use the same id after `sanitize_storage_dir_segment` (hyphens kept). Do not invent ad-hoc
base36 session ids — opaque non-UUID strings are easy for models to mistype in tool args.

## Session sandbox layout

| Case | Path |
| --- | --- |
| Logged-in / IM user id present | `{app_data}/session-sandboxes/{session_user_id}/` |
| Anonymous (no `session_user_id`) | `{app_data}/session-sandboxes/_anonymous/{conversation_id}/` |

That logged-in sandbox path is also the **默认项目** `workspaceRoot` for that user
(see [session-user-id.md](session-user-id.md) Projects section).

- Multiple conversations for the same user **share** one sandbox directory.
- Sandbox directories are created on **first chat run**, not when clearing workspace in the UI.
- **Legacy** `{session-sandboxes}/{conversation_id}/` dirs are deleted on conversation delete only; new runs do not use them.

`pointer-server` (web) uses the same sandbox tree — not a separate `{app_data}/{platform_user_id}/` default.

## Sub-agents (`run_subagent`)

Priority: explicit tool `workspaceRoot` → parent session workspace → default sandbox (`workspace_delegation.rs`).

See [pointer-run-subagent.md](pointer-run-subagent.md).

## IM channels

IM sessions use the same rules. Polluted non-user stored paths (pre-fix inherit bug) are ignored; see `channel_outbound::resolve_im_run_workspace`.

## Cleanup

On conversation delete, Pointer removes:

- Legacy `{session-sandboxes}/{conversation_id}/`
- Anonymous `{session-sandboxes}/_anonymous/{conversation_id}/`

It does **not** remove a shared user sandbox `{session-sandboxes}/{session_user_id}/`.

## AGENTS.md vs session workspace

Each LLM round, Pointer concatenates Codex-style layers (no sibling tree walk, no on-read nested inject) and appends them as system **cacheable** `# Project Context` (Hermes-style) — **not** a per-round user message:

1. **Global** — `~/.pointer/AGENTS.md` (`dirs::home_dir()` / `%USERPROFILE%\.pointer` on Windows). Missing home directory is a warn and skips this layer. Looking up the file does **not** create `~/.pointer`.
2. **Project chain** — session `workspace_root` is **cwd** (not process `cwd`). Walk **up** until a `.git` directory or file is found (git worktrees use a file). Then, from that git root **down the single path** to `workspace_root`, take at most one `AGENTS.md` per directory. If there is no git root, only `<workspace>/AGENTS.md`. A filesystem root (`/` / `C:\`) is never used as git root or as a search directory.

Merge order is global first, then git root → workspace (later files override). Duplicate paths (for example the workspace **is** `~/.pointer`) are injected once. `packages/api/AGENTS.md` is ignored when cwd is `packages/web`.

The block uses Hermes wording: files have been loaded and should be followed. Empty workspace still loads the global file. A missing file, an empty file, or a directory named `AGENTS.md` skips that layer (warn when the workspace is not a directory, the path is the filesystem root, or the path exists but is not a file). Read errors are logged and do not fail the round.

`resolve_tool_workspace_root()` (file / terminal / `read_lints` / workspace plugins) uses the conversation override or settings `workspaceRoot` only. It does **not** fall back to process `cwd`.

Logs:

- `agents_md: system cacheable inject enabled (global ~/.pointer/AGENTS.md + git-root-to-workspace chain)` at process start
- `agents_md: load conversation_id=... workspace=... git_root=... elapsed_ms=... files=... labels=... injected=... partition=cacheable` on each prompt round (`warn` if `elapsed_ms >= 1000`)
- Per-hook and other pre-stream phases: `phase_timing: phase=... conversation_id=... elapsed_ms=...`
- After HTTP POST: `stream_chat: http_until_headers_ms=...` then `stream_chat: first_token_ms=...`

## Related

- [session-user-id.md](session-user-id.md) — how `session_user_id` is set
- [terminal-environment-variables.md](terminal-environment-variables.md) — `WORKING_DIR` / `DATA_DIR` / `SKILL_DIR`
- [file-tool-write-scope.md](file-tool-write-scope.md) — write scope vs workspace root
