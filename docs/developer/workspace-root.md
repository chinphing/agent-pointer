# Session workspace root

The **workspace root** is the directory used by `file`, `terminal`, `read_lints`, and related tools for a conversation. It is persisted as `workspaceRoot` on the conversation row and exposed to scripts as `WORKING_DIR` (see [terminal-environment-variables.md](terminal-environment-variables.md)).

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

## Session sandbox layout

| Case | Path |
| --- | --- |
| Logged-in / IM user id present | `{app_data}/session-sandboxes/{session_user_id}/` |
| Anonymous (no `session_user_id`) | `{app_data}/session-sandboxes/_anonymous/{conversation_id}/` |

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

## Related

- [session-user-id.md](session-user-id.md) — how `session_user_id` is set
- [terminal-environment-variables.md](terminal-environment-variables.md) — `WORKING_DIR`
- [file-tool-write-scope.md](file-tool-write-scope.md) — write scope vs workspace root
