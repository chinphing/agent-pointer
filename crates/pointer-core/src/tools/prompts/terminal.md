### `terminal`

Run a shell command and return console output.

Use when you need the shell for builds, checks, tests, directory listings, or other CLI work in the project.

**Guidelines**

- Decide whether a terminal is truly needed; prefer answering from context when possible.
- Prefer running inside the workspace; set **`cwd`** explicitly when required.
- On Windows, commands run via `powershell -NoProfile -ExecutionPolicy Bypass -Command`; keep commands portable when you can.
- Set a reasonable **`timeoutMs`** for long-running commands.
- Do not run destructive commands unless the user clearly asked and approval allows it.
- Do not read or exfiltrate secrets via the terminal.

The result includes `stdout`, `stderr`, exit code, timing, and truncation flags; when replying, summarize only output relevant to the task.

#### Parameters

- **`command`** (required) — The shell command to run. Windows: `powershell -NoProfile -ExecutionPolicy Bypass -Command`; macOS/Linux: `sh -lc`.
- **`cwd`** (optional) — Working directory; must be an existing directory.
- **`timeoutMs`** (optional) — Timeout in milliseconds; default **30000**, max **120000**.
- **`maxOutputBytes`** (optional) — Max bytes per stream for stdout and stderr; default **20000**, max **200000**; output is truncated per stream when exceeded.

#### XML example

```xml
<response>
  <thoughts>Run tests in the workspace.</thoughts>
  <headline>Cargo test</headline>
  <tool_name>terminal</tool_name>
  <tool_args>
    <command>cargo test</command>
    <cwd>.</cwd>
    <timeoutMs>120000</timeoutMs>
  </tool_args>
</response>
```
