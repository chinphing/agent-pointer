Use `terminal` when you need shell commands for builds, checks, tests, file listings, or other CLI work inside the project.

Rules:
- Decide whether a terminal is truly needed; answer from context when possible.
- Prefer running inside the workspace; set `cwd` explicitly when required.
- On Windows, commands run via `powershell -NoProfile -ExecutionPolicy Bypass -Command`; keep commands portable when you can.
- Set a reasonable `timeoutMs` for long-running commands.
- Do not run destructive commands unless the user clearly asked and approval allows it.
- Do not read or exfiltrate secrets via the terminal.

The result includes `stdout`, `stderr`, exit code, timing, and truncation flags; summarize only output relevant to the task when replying.
