# Terminal environment variables

Pointer injects session-scoped variables into **`terminal`** child processes only.
The host process and other tools are unchanged.

Child env is built in `build_terminal_child_environment` (process env + optional `.env` overlays + session vars below).

## Variables

| Variable | Set when | Value |
| --- | --- | --- |
| `WORKING_DIR` | Session workspace root is non-empty | Absolute or normalized path to the conversation workspace root (same root as default **`terminal`** **`cwd`**) |
| `SESSION_USER_ID` | Stored conversation `session_user_id` is non-empty | Persisted session user id (see [session-user-id.md](session-user-id.md)) |

Both are omitted when the corresponding value is empty.

## Resolution

### `WORKING_DIR`

1. **`run_chat` workspace** — user-selected project folder or default session sandbox (`session-sandboxes/{session_user_id}/` or `_anonymous/{conversation_id}/`). See [workspace-root.md](workspace-root.md).
2. **Per tool call** — for **`terminal`**, the runtime resolves workspace again on the blocking thread (settings override → conversation row → default sandbox), same as default **`cwd`**, then sets thread-local context before spawning the shell.

`WORKING_DIR` matches that resolved session workspace, not an arbitrary **`terminal`** **`cwd`** argument. If the model passes **`cwd`**, the shell starts in that directory but `WORKING_DIR` still points at the session workspace root.

### `SESSION_USER_ID`

See [session-user-id.md](session-user-id.md) for how the id is chosen and persisted.

## Thread-local guards

During an agent run, Pointer sets thread-local `WORKING_DIR` / `SESSION_USER_ID` context on:

- The main **`run_chat`** thread (`session_inner`)
- Each registry tool invoke thread (`agent_tool_pass` dispatch)
- Each **`terminal`** blocking worker (re-read from conversation store / resolved workspace before spawn)

This mirrors why **`terminal`** re-establishes workspace on the blocking pool: tokio worker threads do not inherit earlier thread-locals.

## Usage in scripts and Skills

```bash
# Example: run a repo script relative to the session workspace
"$WORKING_DIR/scripts/check.sh"

# Example: gate behavior on IM user (when SESSION_USER_ID is set)
if [ -n "$SESSION_USER_ID" ]; then
  echo "session user: $SESSION_USER_ID"
fi
```

Prefer `WORKING_DIR` over hard-coded paths when a Skill or script must anchor to the active conversation workspace.

## Related docs

- [session-user-id.md](session-user-id.md) — `session_user_id` persistence and IM / desktop resolution
- [workspace-root.md](workspace-root.md) — workspace resolution and sandbox layout
- [file-tool-write-scope.md](file-tool-write-scope.md) — `file_write` / `file_edit` workspace rules
- [`../internals/terminal-shell-path.md`](../internals/terminal-shell-path.md) — shell, `PATH`, and `.env` for **`terminal`**
