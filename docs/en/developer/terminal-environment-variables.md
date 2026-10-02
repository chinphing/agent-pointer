# Terminal environment variables

English | [简体中文](../../zh-CN/developer/terminal-environment-variables.md)

Pointer injects session-scoped variables into **`terminal`** child processes only.
The host process and other tools are unchanged.

Child env is built in `build_terminal_child_environment`:

1. Pointer process env (login-shell / registry `PATH` already merged)
2. Optional `.env` file overlays (`{app_data_dir}/.env` or tool `envFiles`)
3. Windows UTF-8 helpers (`PYTHONUTF8` etc.) when applicable
4. Session `WORKING_DIR` / `SESSION_USER_ID` / host `DATA_DIR` / `SKILL_DIR`
5. **Settings `terminalEnvOverrides`** (Settings → Runtime → Terminal environment; valid for this session)
6. Unix UTF-8 locale (`unix_locale`) — Dock-launched apps often inherit `LANG=C`, which makes PTY `ls` print `?` for Chinese names; applied last so overrides cannot leave a non-UTF-8 locale

Non-`PATH` keys in steps 2 and 5 **override** earlier values for the child only. `PATH` is **prepended**. Step 6 may still correct `LANG` / `LC_*` to UTF-8.

## Variables

| Variable | Set when | Value |
| --- | --- | --- |
| `WORKING_DIR` | Session workspace root is non-empty | Absolute or normalized path to the conversation workspace root (same root as default **`terminal`** **`cwd`**) |
| `SESSION_USER_ID` | Stored conversation `session_user_id` is non-empty | Persisted session user id (see [session-user-id.md](../../zh-CN/developer/session-user-id.md)) |
| `DATA_DIR` | App data directory resolves successfully | Absolute path to Pointer app data root (the directory holding `conversations.db`, `session-sandboxes/`, `skills/`, etc.; consistent with `POINTER_APP_DATA_DIR` / the default `PointerApp`/`PointerAppDev`) |
| `SKILL_DIR` | User skill library root resolves successfully | Absolute path to the user skill library (`~/.pointer/skills`; consistent with `skills::external::pointer_skills_dir()`) |

All are omitted when the corresponding value is empty / unavailable.

## Settings overrides (`terminalEnvOverrides`)

Enable **Debug mode** → Settings → **Runtime** → **Terminal environment**, add `KEY` / `VALUE`, then click "Save (this session)".

- Affects only `terminal` (including elevated / PTY) child processes; it does not change the Pointer host process.
- Can override keys injected by the process / `.env` / session (including `WORKING_DIR`, `SESSION_USER_ID`, `DATA_DIR`, `SKILL_DIR`).
- Sensitive keys (including `API_KEY` / `SECRET`, etc.) are skipped.
- After **Debug mode** is turned off, injection **still continues**; the edit entry is still under the debug **Runtime** section, so turning it on again lets you change the configuration.
- Not written to disk; defaults are restored after a restart.

## Resolution

### `WORKING_DIR`

1. **`run_chat` workspace** — user-selected project folder or default session sandbox (`session-sandboxes/{session_user_id}/` or `_anonymous/{conversation_id}/`). See [workspace-root.md](../../zh-CN/developer/workspace-root.md).
2. **Per tool call** — for **`terminal`**, the runtime resolves workspace again on the blocking thread (settings override → conversation row → default sandbox), same as default **`cwd`**, then sets thread-local context before spawning the shell.

`WORKING_DIR` matches that resolved session workspace, not an arbitrary **`terminal`** **`cwd`** argument. If the model passes **`cwd`**, the shell starts in that directory but `WORKING_DIR` still points at the session workspace root.

### `SESSION_USER_ID`

See [session-user-id.md](../../zh-CN/developer/session-user-id.md) for how the id is chosen and persisted.

**Server optional guard (Agent `terminal` only):** set
`[server].forbid_session_user_id_in_terminal = true` in `pointer-server.toml`
(or `POINTER_SERVER_FORBID_SESSION_USER_ID_IN_TERMINAL=true`). When enabled, the
server rejects tool calls whose `command` / `stdin` contain the literal
`SESSION_USER_ID` string. Default **off**. Desktop / Tauri does not use this
option. Workspace Console PTY is unaffected. This is a weak tool-path guard, not
a security boundary — identity-sensitive logic must still trust the conversation
row / host injection.

### `DATA_DIR`

Always the host **app data directory** resolved by `storage::app_data_dir()`:

1. `POINTER_APP_DATA_DIR` when set and non-empty
2. Otherwise `{OS data dir}/{POINTER_APP_DATA_SUBDIR|PointerApp|PointerAppDev}`

Unlike `WORKING_DIR`, this is **not** conversation-scoped. Scripts that need attachments, sandboxes, or other app-local paths should prefer `"$DATA_DIR/..."` over hard-coded OS paths.

### `SKILL_DIR`

Always the **user skill library root** resolved by `skills::external::pointer_skills_dir()`:

- `~/.pointer/skills` (`pointer_home_dir()/skills`), shared across app installs, maintained automatically by the curator.

Like `DATA_DIR`, this is **not** conversation-scoped. Skill scripts that need to locate sibling skills or the shared user library should prefer `"$SKILL_DIR/..."` over hard-coded home paths.

## Thread-local guards

During an agent run, Pointer sets thread-local `WORKING_DIR` / `SESSION_USER_ID` context on:

- The main **`run_chat`** thread (`session_inner`)
- Each registry tool invoke thread (`agent_tool_pass` dispatch)
- Each **`terminal`** blocking worker (re-read from conversation store / resolved workspace before spawn)

`DATA_DIR` / `SKILL_DIR` do not use a thread-local; they are resolved from host storage when building the child env.

This mirrors why **`terminal`** re-establishes workspace on the blocking pool: tokio worker threads do not inherit earlier thread-locals.

## Usage in scripts and Skills

```bash
# Example: run a repo script relative to the session workspace
"$WORKING_DIR/scripts/check.sh"

# Example: read something under app data (attachments / sandboxes layout)
ls "$DATA_DIR/session-sandboxes"

# Example: list the user skill library (shared across app installs)
ls "$SKILL_DIR"

# Example: gate behavior on IM user (when SESSION_USER_ID is set)
if [ -n "$SESSION_USER_ID" ]; then
  echo "session user: $SESSION_USER_ID"
fi
```

Prefer `WORKING_DIR` for the active conversation workspace, `DATA_DIR` for Pointer app-local storage, and `SKILL_DIR` for the user skill library.

## Related docs

- [session-user-id.md](../../zh-CN/developer/session-user-id.md) — `session_user_id` persistence and IM / desktop resolution
- [workspace-root.md](../../zh-CN/developer/workspace-root.md) — workspace resolution and sandbox layout
- [file-tool-write-scope.md](../../zh-CN/developer/file-tool-write-scope.md) — `file_write` / `file_edit` workspace rules
- [terminal-interactive-input.md](../../zh-CN/developer/terminal-interactive-input.md) — SSH/PTY password and yes/no modal
- [`terminal-shell-path.md`](../../zh-CN/internals/terminal-shell-path.md) — shell, `PATH`, and `.env` for **`terminal`**
