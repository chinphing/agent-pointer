### `terminal`

Run a shell command and return console output.

Use when you need the shell for builds, checks, tests, directory listings, or other CLI work in the project.

**Guidelines**

- Decide whether a terminal is truly needed; prefer answering from context when possible.
- Prefer running inside the workspace; set **`cwd`** explicitly when required.
- On Windows, commands run via `powershell -NoProfile -ExecutionPolicy Bypass -Command`; keep commands portable when you can.
- Set a reasonable **`timeoutMs`** (idle: no stdout/stderr resets the timer; total wall time is still capped).
- Do not run destructive commands unless the user clearly asked and approval allows it.
- Do not read or exfiltrate secrets via the terminal.

The result includes `stdout`, `stderr`, exit code, timing, and truncation flags; when replying, summarize only output relevant to the task.

#### Parameters

- **`command`** (required) — The shell command to run. Windows: `powershell -NoProfile -ExecutionPolicy Bypass -Command`; macOS/Linux: `sh -lc`.
- **`cwd`** (optional) — Working directory; must be an existing directory.
- **`timeoutMs`** (optional) — Idle timeout in milliseconds: if neither stdout nor stderr receives new data for this long, the command is stopped. Any new output resets this idle timer. Default **30000**, per-idle segment max **120000**. Regardless of output, the command cannot run longer than **1 hour** from start.
- **`maxOutputBytes`** (optional) — Max bytes per stream for stdout and stderr; default **20000**, max **200000**; output is truncated per stream when exceeded.

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
