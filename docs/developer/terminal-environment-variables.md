# Terminal environment variables

Pointer injects session-scoped variables into **`terminal`** child processes only.
The host process and other tools are unchanged.

Child env is built in `build_terminal_child_environment`:

1. Pointer process env (login-shell / registry `PATH` already merged)
2. Optional `.env` file overlays (`{app_data_dir}/.env` or tool `envFiles`)
3. Session `WORKING_DIR` / `SESSION_USER_ID`
4. **Settings `terminalEnvOverrides`** (settings → 界面配置 → 终端环境变量；本次会话有效)

Non-`PATH` keys in steps 2 and 4 **override** earlier values for the child only. `PATH` is **prepended**. Overrides in step 4 win last (including over session vars).

## Variables

| Variable | Set when | Value |
| --- | --- | --- |
| `WORKING_DIR` | Session workspace root is non-empty | Absolute or normalized path to the conversation workspace root (same root as default **`terminal`** **`cwd`**) |
| `SESSION_USER_ID` | Stored conversation `session_user_id` is non-empty | Persisted session user id (see [session-user-id.md](session-user-id.md)) |

Both are omitted when the corresponding value is empty.

## Settings overrides (`terminalEnvOverrides`)

Enable **调试模式** → 设置 → **界面配置** → **终端环境变量**，添加 `KEY` / `VALUE` 后点「保存(本次会话)」。

- 仅影响 `terminal`（含 elevated / PTY）子进程，不改 Pointer 主机进程。
- 可覆盖进程 / `.env` / 会话注入的键（含 `WORKING_DIR`、`SESSION_USER_ID`）。
- 敏感键（含 `API_KEY` / `SECRET` 等）会被跳过。
- 关闭调试模式后**仍继续注入**；编辑入口仍在调试「界面配置」中，再次开启可改配置。
- 不写入磁盘；重启后恢复默认。

## Resolution

### `WORKING_DIR`

1. **`run_chat` workspace** — user-selected project folder or default session sandbox (`session-sandboxes/{session_user_id}/` or `_anonymous/{conversation_id}/`). See [workspace-root.md](workspace-root.md).
2. **Per tool call** — for **`terminal`**, the runtime resolves workspace again on the blocking thread (settings override → conversation row → default sandbox), same as default **`cwd`**, then sets thread-local context before spawning the shell.

`WORKING_DIR` matches that resolved session workspace, not an arbitrary **`terminal`** **`cwd`** argument. If the model passes **`cwd`**, the shell starts in that directory but `WORKING_DIR` still points at the session workspace root.

### `SESSION_USER_ID`

See [session-user-id.md](session-user-id.md) for how the id is chosen and persisted.

**Server optional guard (Agent `terminal` only):** set
`[server].forbid_session_user_id_in_terminal = true` in `pointer-server.toml`
(or `POINTER_SERVER_FORBID_SESSION_USER_ID_IN_TERMINAL=true`). When enabled, the
server rejects tool calls whose `command` / `stdin` contain the literal
`SESSION_USER_ID` string. Default **off**. Desktop / Tauri does not use this
option. Workspace Console PTY is unaffected. This is a weak tool-path guard, not
a security boundary — identity-sensitive logic must still trust the conversation
row / host injection.

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
- [terminal-interactive-input.md](terminal-interactive-input.md) — SSH/PTY password and yes/no modal
- [`../internals/terminal-shell-path.md`](../internals/terminal-shell-path.md) — shell, `PATH`, and `.env` for **`terminal`**
