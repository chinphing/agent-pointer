# Terminal tool: shell PATH (macOS / Linux / Windows)

## macOS & Linux

On app startup, `AppState::new()` calls `shell_env::bootstrap_process_path_from_login_shell()`:

- Runs `$SHELL -ilc 'printf %s "$PATH"'` once (fallback shell: `/bin/zsh` on macOS, `/bin/bash` on Linux).
- Merges the login-shell `PATH` into the Pointer process environment.
- All `terminal` / `read_lints` subprocesses inherit the merged `PATH`; `terminal` still uses `sh -lc` (no profile per command).

Implementation: `crates/pointer-core/src/shell_env.rs`.

## Windows

The `terminal` tool runs `powershell -ExecutionPolicy Bypass -Command` **with the user PowerShell profile loaded** (no `-NoProfile`), so fnm/nvm-style PATH hooks in `$PROFILE` apply per command.

Tradeoff: slower and profile-dependent; see product notes in agent discussions.

## Supplementary `.env` files

User-managed environment variables live in **`{app_data_dir}/.env`** (`storage::user_env_file_path()`). When the `terminal` tool call omits **`envFiles`**, that file is loaded automatically if it exists.

Optional **`envFiles`** overrides the default and loads only the listed paths (for project-specific `.env` files). Each file is parsed as dotenv (`KEY=VALUE`, optional `export`, `#` comments, quoted values).

Merge rules:

1. The subprocess inherits the Pointer process environment (including the merged login-shell `PATH` on Unix).
2. Variables from `.env` files are applied **only for keys not already set** in that inherited environment.
3. When multiple files are listed, later files override earlier ones within the supplemental layer.

Relative paths resolve against the effective **`cwd`** (or workspace root when **`cwd`** is omitted). Absolute paths are allowed when the file exists.

Implementation: `crates/pointer-core/src/dotenv.rs`, wired in `tools/terminal.rs`.
