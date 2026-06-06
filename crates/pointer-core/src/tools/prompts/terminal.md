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
- On Windows, commands run via `powershell -ExecutionPolicy Bypass -Command` (user PowerShell profile is loaded so fnm/nvm PATH hooks apply); keep commands portable when you can.
- Set a reasonable **`timeoutMs`** (idle: no stdout/stderr resets the timer). Use **`maxWallMs`** when you need a shorter hard wall than the default cap.
- Do not run destructive commands unless the user clearly asked and approval allows it.
- Do not read or exfiltrate secrets via the terminal.
- **Default user env:** When **`envFiles`** is omitted, the host loads the app data directory **`.env`** (see **App data directory** in session context) if that file exists. Users store personal / cross-project vars there (e.g. API keys).
- **Project env:** When a command needs workspace-specific vars, pass **`envFiles`** with paths to project `.env` files. Variables from loaded files are **supplementary** env: they apply only when the host process does not already define the same key. Later entries in **`envFiles`** override earlier ones within that supplemental layer. Paths may be absolute or relative to **`cwd`** (or the workspace root when **`cwd`** is omitted).

The result includes `stdout`, `stderr`, exit code, timing, truncation flags, **`envFiles`** (resolved paths that were loaded), and **`cancelled`** when the host stopped the turn; it may set **`runAborted`: true** when only this command was stopped and the turn continues—treat that as an interrupted run, not a full turn cancel. When replying, summarize only output relevant to the task.

#### Parameters

- **`command`** (required) — The shell command to run. Windows: `powershell -ExecutionPolicy Bypass -Command` (profile loaded); macOS/Linux: `sh -lc`.
- **`cwd`** (optional) — Working directory; must be an existing directory.
- **`timeoutMs`** (optional) — Idle timeout in milliseconds: if neither stdout nor stderr receives new data for this long, the command is stopped. Any new output resets this idle timer. Default **30000**, per-idle segment max **120000**.
- **`maxWallMs`** (optional) — Hard wall-clock cap from process start; when elapsed time reaches this value, the process tree is killed. Min **1000**, max **3600000** (1 hour). Omit to use the **1 hour** default cap.
- **`maxOutputBytes`** (optional) — Max bytes per stream for stdout and stderr; default **20000**, max **200000**; output is truncated per stream when exceeded.
- **`envFiles`** (optional) — One `.env` path (string) or an array of paths; processed in order. When omitted, the default app-data **`.env`** is loaded if it exists.

#### Stopping the run (host)

When the user **stops generation** or the host **cancels the turn**, the subprocess is **force-terminated** (Windows: `taskkill /T /F` on the shell PID, then `Child::kill`; macOS/Linux: `Child::kill` on the shell). The tool result sets **`cancelled`: true** and a short notice is appended to streamed output.

The host may instead **abort only the current terminal subprocess** while the assistant turn keeps going; then **`runAborted`** is set (not **`cancelled`**).
