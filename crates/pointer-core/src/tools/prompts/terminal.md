---
schema:
  type: object
  properties:
    command:
      type: string
    cwd:
      type: string
    timeoutMs:
      type: integer
      minimum: 1000
    maxWallMs:
      type: integer
      minimum: 1000
    maxOutputBytes:
      type: integer
      minimum: 1
    envFiles:
      oneOf:
        - type: string
        - type: array
          items:
            type: string
    elevated:
      type: boolean
  required:
    - command
  additionalProperties: true
---

### `terminal`

Run a shell command and return console output.

Use when you need the shell for builds, checks, tests, directory listings, or other CLI work in the project.

**Guidelines**

- Decide whether a terminal is truly needed; prefer answering from context when possible.
- Prefer running inside the workspace; set **`cwd`** explicitly when required.
- **Path discipline:** Do **not** put `cd /some/absolute/path` in **`command`** unless that path is the injected **workspace root**, a path returned by **`file`** / prior **`terminal`** output in **this** session, or the user pasted it verbatim. **Never invent** repo paths from memory or project names.
- For git in the current project: omit `cd` (default cwd is the workspace root) or run **`git -C "<workspace_root>" …`** using the root from session context—not a guessed path.
- **Windows shell (you choose):** The host picks the wrapper from your **`command`** text:
  - **Default** — no `cmd` / `powershell` / `pwsh` prefix → host runs **`powershell -ExecutionPolicy Bypass -Command`** (user PowerShell profile loaded). The host prepends UTF-8 **`OutputEncoding`** setup so console output is not limited to the system OEM code page.
  - **Explicit CMD** — start with **`cmd.exe /c "…"`** or **`cmd /c "…"`** → host parses `/c` and runs a **single** `cmd.exe /C <script>` (Command Prompt semantics; safe for `%PATH%`, `2>nul`, `||`). The host prefixes **`chcp 65001`** on the script. Use for batch-style tools, `2>&1`, or when PowerShell aliases/redirects get in the way.
  - **Explicit PowerShell** — start with **`powershell …`** or **`pwsh …`** → host spawns **`powershell.exe` / `pwsh.exe` directly** with your flags (no outer `cmd.exe /C`; safe for `$env:Path` and mixed quoting). When you pass **`-Command` / `-c`**, the host prepends the same UTF-8 **`OutputEncoding`** setup as the default wrapper.
  - **Do not double-wrap** — never prefix a command that already starts with `cmd` / `powershell` / `pwsh`.
  - When unsure on Windows, prefer **`cmd.exe /c "…"`** for simple CLI checks (`python --version`, `npm -v`, `git status`).
- Do not run destructive commands unless the user clearly asked and approval allows it.
- Do not read or exfiltrate secrets via the terminal.
- Use **`elevated`: true** only when the task clearly needs administrator / root privileges (e.g. system-wide install, protected paths). The host always requires **in-app approval** first; the OS may show a second prompt (UAC / admin password / polkit). Do not set **`elevated`** for ordinary project commands.
- With **`elevated`: true**, do **not** prefix the command with **`sudo`**, **`pkexec`**, or Windows **`Start-Process -Verb RunAs`** — the host elevates once. Combine multiple admin steps in **one** call with **`&&`** (e.g. `apt update && apt install -y pkg`) instead of several separate **`elevated`** calls; **each** elevated call triggers a **new** OS password / UAC prompt.
- **Default user env:** When **`envFiles`** is omitted, the host loads the app data directory **`.env`** (see **App data directory** in session context) if that file exists. Users store personal / cross-project vars there (e.g. API keys).
- **Project env:** When a command needs workspace-specific vars, pass **`envFiles`** with paths to project `.env` files. Loaded vars apply to the **child shell only** (non-`PATH` keys override inherited values; `PATH` is prepended before inherited `PATH`). Later entries in **`envFiles`** override earlier ones. Paths may be absolute or relative to **`cwd`** (or the workspace root when **`cwd`** is omitted).
The result includes `stdout`, `stderr`, exit code, timing, truncation flags, **`envFiles`** (resolved paths that were loaded), and **`cancelled`** when the host stopped the turn; it may set **`runAborted`: true** when only this command was stopped and the turn continues—treat that as an interrupted run, not a full turn cancel. When replying, summarize only output relevant to the task.

#### Timeouts

Either limit can stop the process (**`timedOut`** in the result). Set both per call when needed.

- **`timeoutMs`** — **Idle** timeout: if stdout/stderr get no **new** data for this long, the run is killed. Any new output **resets** the idle timer. Default **30000**, max **300000** (5 min).
- **`maxWallMs`** — **Wall clock** from process start, with or without output. Default **3600000** (1 h); max **3600000**. Pass a lower value only when you need a shorter hard cap.

**Choosing values:** Defaults suit quick commands. Steady log output keeps resetting idle time — you usually only need a custom **`maxWallMs`**.

**Human-in-the-loop:** Shell waits for the user (password, MFA, `npm login`, interactive installer) may produce **no output for minutes**. Set **`timeoutMs`** toward **300000** and **`maxWallMs`** for the expected wait; tell the user what to do in assistant **`content`** while the command runs.

#### Parameters

- **`command`** (required) — The shell command to run. Windows: default **PowerShell** wrapper; `cmd` / `powershell` / `pwsh` prefixes spawn that shell **directly** (no outer `cmd.exe /C`). macOS/Linux: `sh -lc`.
- **`cwd`** (optional) — Working directory; must be an existing directory.
- **`timeoutMs`** (optional) — See **Timeouts**.
- **`maxWallMs`** (optional) — See **Timeouts**.
- **`maxOutputBytes`** (optional) — Max bytes per stream for stdout and stderr; default **20000**, max **200000**; output is truncated per stream when exceeded.
- **`envFiles`** (optional) — One `.env` path (string) or an array of paths; processed in order. When omitted, the default app-data **`.env`** is loaded if it exists.
- **`elevated`** (optional) — When **true**, run with administrator / root privileges after **in-app** user approval. Windows: UAC; macOS: admin password; Linux: `pkexec` (polkit). Output is collected after the command finishes (no live stream). Result may include **`elevationDenied`** when the user declines the OS prompt.

#### Stopping the run (host)

When the user **stops generation** or the host **cancels the turn**, the subprocess is **force-terminated** (Windows: `taskkill /T /F` on the shell PID, then `Child::kill`; macOS/Linux: `Child::kill` on the shell). The tool result sets **`cancelled`: true** and a short notice is appended to streamed output.

The host may instead **abort only the current terminal subprocess** while the assistant turn keeps going; then **`runAborted`** is set (not **`cancelled`**).
