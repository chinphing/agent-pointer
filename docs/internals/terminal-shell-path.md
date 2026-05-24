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
