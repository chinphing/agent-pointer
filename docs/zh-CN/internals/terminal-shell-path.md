# 终端工具：shell PATH（macOS / Linux / Windows）

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
| (none) | `powershell -ExecutionPolicy Bypass -Command` (profile loaded); UTF-8 `InputEncoding` / `OutputEncoding` preamble on the command |
| `cmd` / `cmd.exe` | **Direct** `cmd.exe /C <script>` — no outer `cmd /C` wrapper; script prefixed with `chcp 65001>nul &` |
| `powershell` / `pwsh` | **Direct** `powershell.exe` / `pwsh.exe` with parsed argv — no outer `cmd /C`; `-Command` / `-c` body gets UTF-8 preamble |

Prompt: `tools/prompts/terminal.md` tells the model to prefix **`cmd.exe /c "…"`** when CMD semantics are needed. Profile-only PATH hooks still require registry/`Path`, startup merge, or `{app_data_dir}/.env`.

## Supplementary `.env` files

User-managed environment variables live in **`{app_data_dir}/.env`** (`storage::user_env_file_path()`). When the `terminal` tool call omits **`envFiles`**, that file is loaded automatically if it exists.

Optional **`envFiles`** overrides the default and loads only the listed paths (for project-specific `.env` files). Each file is parsed as dotenv (`KEY=VALUE`, optional `export`, `#` comments, quoted values).

Merge rules:

1. Start from the **Pointer process environment** (including merged login-shell / registry `PATH`).
2. Non-`PATH` variables from `.env` **override** those values for the child only (host unchanged).
3. `PATH` from `.env` is **prepended** before the inherited `PATH` (deduplicated). `%PATH%` / `$PATH` in the `.env` value is expanded before merge.
4. When multiple files are listed, later files override earlier ones within the merged layer.
5. Session `WORKING_DIR` / `SESSION_USER_ID` / host `DATA_DIR` / `SKILL_DIR` are injected next.
6. Settings **`terminalEnvOverrides`** (settings UI, session memory; independent of debug menus) apply last with the same override / PATH-prepend rules (can override session vars).

Non-elevated and **elevated** (`elevated: true`) terminal runs both use the same builder (`build_terminal_child_environment`): full process env + `.env` + debug overlays. Elevated hosts inject that map into the admin/root child (Windows `env.json`; Unix `env.sh` + `set -a`).

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
4. **Environment** matches non-elevated runs (same `build_terminal_child_environment` map). Windows elevated PowerShell still uses `-NoProfile`; Unix elevated shells do not load login profiles — only explicit env injection + system defaults for keys not in the Pointer process.
5. **Windows encoding** — non-elevated runs: default PowerShell wrapper and explicit `-Command` / `-c` get a UTF-8 `InputEncoding` / `OutputEncoding` preamble; `cmd.exe` scripts get `chcp 65001>nul &`. Piped stdout/stderr are read as bytes and decoded with a shared UTF-8 stream decoder. Child env includes `PYTHONIOENCODING=utf-8`, `PYTHONUTF8=1`, and `JAVA_TOOL_OPTIONS=-Dfile.encoding=UTF-8` when unset. Elevated runs use the same CMD prefix and UTF-8 setup in `job.ps1`; stdout/stderr capture files are written as **UTF-8 (no BOM)** when possible.
6. **Windows `cwd`** — `canonicalize()` may yield `\\?\` extended paths; `terminal` strips that prefix before setting child / elevated `WorkingDirectory` so `cmd.exe` does not emit UNC cwd warnings.

Implementation: `crates/pointer-core/src/tools/terminal_elevated.rs`, wired from `tools/terminal.rs`.
