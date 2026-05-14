### `terminal`

Run a shell command and return console output.

Use when you need the shell for builds, checks, tests, directory listings, or other CLI work in the project.

**Guidelines**

- Decide whether a terminal is truly needed; prefer answering from context when possible.
- Prefer running inside the workspace; set **`cwd`** explicitly when required.
- On Windows, commands run via `powershell -NoProfile -ExecutionPolicy Bypass -Command`; keep commands portable when you can.
- Set a reasonable **`timeoutMs`** (idle: no stdout/stderr resets the timer). Use **`maxWallMs`** when you need a shorter hard wall than the default cap.
- Do not run destructive commands unless the user clearly asked and approval allows it.
- Do not read or exfiltrate secrets via the terminal.

The result includes `stdout`, `stderr`, exit code, timing, truncation flags, and **`cancelled`** when the host stopped the turn; it may set **`runAborted`: true** when only this command was stopped and the turn continues—treat that as an interrupted run, not a full turn cancel. When replying, summarize only output relevant to the task.

#### Parameters

- **`command`** (required) — The shell command to run. Windows: `powershell -NoProfile -ExecutionPolicy Bypass -Command`; macOS/Linux: `sh -lc`.
- **`cwd`** (optional) — Working directory; must be an existing directory.
- **`timeoutMs`** (optional) — Idle timeout in milliseconds: if neither stdout nor stderr receives new data for this long, the command is stopped. Any new output resets this idle timer. Default **30000**, per-idle segment max **120000**.
- **`maxWallMs`** (optional) — Hard wall-clock cap from process start; when elapsed time reaches this value, the process tree is killed. Min **1000**, max **3600000** (1 hour). Omit to use the **1 hour** default cap.
- **`maxOutputBytes`** (optional) — Max bytes per stream for stdout and stderr; default **20000**, max **200000**; output is truncated per stream when exceeded.

#### Stopping the run (host)

When the user **stops generation** or the host **cancels the turn**, the subprocess is **force-terminated** (Windows: `taskkill /T /F` on the shell PID, then `Child::kill`; macOS/Linux: `Child::kill` on the shell). The tool result sets **`cancelled`: true** and a short notice is appended to streamed output.

The host may instead **abort only the current terminal subprocess** while the assistant turn keeps going; then **`runAborted`** is set (not **`cancelled`**).

#### JSON example

```json
{
  "thoughts": "Run tests in the workspace.",
  "headline": "Cargo test",
  "tool_name": "terminal",
  "tool_args": {
    "command": "cargo test",
    "cwd": ".",
    "timeoutMs": 120000
  }
}
```
