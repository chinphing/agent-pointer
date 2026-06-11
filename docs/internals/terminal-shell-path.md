# Terminal tool: shell PATH (macOS / Linux / Windows)

## macOS & Linux

On app startup, `AppState::new()` calls `shell_env::bootstrap_process_path_from_login_shell()`:

- Runs `$SHELL -ilc 'printf %s "$PATH"'` once (fallback shell: `/bin/zsh` on macOS, `/bin/bash` on Linux).
- Merges the login-shell `PATH` into the Pointer process environment.
- All `terminal` / `read_lints` subprocesses inherit the merged `PATH`; `terminal` still uses `sh -lc` (no profile per command).

Implementation: `crates/pointer-core/src/shell_env.rs`.

## Windows

On app startup, `AppState::new()` calls `shell_env::bootstrap_process_path_from_login_shell()`. Before each **`terminal`** tool run on Windows, `shell_env::refresh_process_path_from_registry()` runs the same merge again (native registry read via `winreg`, under ~1 ms).

- Reads **Machine + User** `Path` from the registry (`HKLM\...\Environment`, `HKCU\Environment`) and merges into the Pointer process `PATH`.
- Moves `WindowsApps` app-execution-alias entries (e.g. Store `python.exe` stub) to the **end** so a real install wins.
- Installing or changing system `Path` while Pointer is open takes effect on the **next** terminal command (no app restart).

The `terminal` tool picks the Windows wrapper from the model-supplied **`command`**:

| Command prefix | Host behavior |
|----------------|---------------|
| (none) | `powershell -ExecutionPolicy Bypass -Command` (profile loaded) |
| `cmd` / `cmd.exe` / `powershell` / `pwsh` | `cmd.exe /C <command>` as-is — no second wrapper |

Prompt: `tools/prompts/terminal.md` tells the model to prefix **`cmd.exe /c "…"`** when CMD semantics are needed. Profile-only PATH hooks still require registry/`Path`, startup merge, or `{app_data_dir}/.env`.

## Supplementary `.env` files

User-managed environment variables live in **`{app_data_dir}/.env`** (`storage::user_env_file_path()`). When the `terminal` tool call omits **`envFiles`**, that file is loaded automatically if it exists.

Optional **`envFiles`** overrides the default and loads only the listed paths (for project-specific `.env` files). Each file is parsed as dotenv (`KEY=VALUE`, optional `export`, `#` comments, quoted values).

Merge rules:

1. The subprocess inherits the Pointer process environment (including the merged login-shell `PATH` on Unix).
2. Non-`PATH` variables from `.env` **override** the inherited value for the child only (host process unchanged).
3. `PATH` from `.env` is **prepended** before the inherited `PATH` (deduplicated). `%PATH%` / `$PATH` in the `.env` value is expanded before merge.
4. When multiple files are listed, later files override earlier ones within the merged layer.

Relative paths resolve against the effective **`cwd`** (or workspace root when **`cwd`** is omitted). Absolute paths are allowed when the file exists.

Implementation: `crates/pointer-core/src/dotenv.rs`, wired in `tools/terminal.rs`.

## Elevated execution (`elevated: true`)

When the model passes **`elevated`: true** on a `terminal` tool call:

1. **In-app approval** is required even if tool approval mode is **auto** (same `pending_approval` UI as manual mode).
2. After approval, the host runs the command with OS elevation:
   - **Windows** — `Start-Process -Verb RunAs` (UAC).
   - **macOS** — `osascript` `do shell script … with administrator privileges`.
   - **Linux** — `pkexec sh -lc …` (requires polkit).
3. Live stdout/stderr streaming is **not** available; output is returned when the elevated process exits. The result JSON may include **`elevationDenied`** if the user declines the OS prompt.

Implementation: `crates/pointer-core/src/tools/terminal_elevated.rs`, wired from `tools/terminal.rs`.
