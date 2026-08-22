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
    stdin:
      type: string
    waitForInputMs:
      type: integer
      minimum: 1000
    envFiles:
      oneOf:
        - type: string
        - type: array
          items:
            type: string
    elevated:
      type: boolean
    label:
      type: string
  required:
    - command
  additionalProperties: true
---

### `terminal`

Run a shell command. Returns stdout, stderr, exit code, and timing.

Set **`label`** to a short Chinese description of what the command does (e.g. `运行单元测试`, `安装依赖`). When present the UI shows this label instead of the raw command.

#### When to use

- Builds, tests, git, and other CLI work in the project.
- Prefer answering from context when enough is already known.

#### Working directory and paths

- Default **cwd** is the workspace root. Set **`cwd`** only when needed.
- Do not **`cd`** to paths you invented — only workspace root, paths from
  **`file`** / prior **`terminal`** output, or user-pasted paths.
- Git in this project: omit **`cd`**, or **`git -C "<workspace_root>" …`**
  from session context.

#### Shell

**Windows**

- Omit a prefix — host runs **PowerShell** with UTF-8 setup.
- Prefer PowerShell over Command Prompt; non-ASCII output often garbles under
  `cmd`.
- Use `cmd /c` only when the user asks or the command needs CMD-only syntax
  (`%VAR%`, `2>nul`, `||`, batch).

**macOS / Linux**

- Host runs **`sh -lc`**.

#### Safety

- No destructive commands unless the user clearly asked and approval allows.
- Do not read or exfiltrate secrets via the terminal.
- Summarize only output relevant to the task.

#### Elevation

Set **`elevated`: true** only for admin/root tasks (system install, protected
paths). Requires in-app approval; the OS may prompt again (UAC / password /
polkit).

- Do not add **`sudo`**, **`pkexec`**, or **`RunAs`** — the host elevates once.
- Batch admin steps in **one** call (chain with **`;`** on Windows PowerShell,
  **`&&`** on macOS/Linux).
- Each **`elevated`** call can trigger a new OS prompt.
- No live stream; result may include **`elevationDenied`** if the user declines
  the OS prompt.

#### Environment

- If **`envFiles`** is omitted, the app-data **`.env`** loads when present.
- Pass **`envFiles`** for project vars; later entries override earlier.
- Loaded vars apply to the child shell only; **`PATH`** is prepended.

#### Timeouts

Either limit can stop the process (**`timedOut`** in the result).

- **`timeoutMs`** — idle timeout: no new stdout/stderr for this long → kill.
  New output resets the timer. Default **30000**, max **3600000**.
- **`maxWallMs`** — wall clock from process start. Default **3600000**.

Interactive waits (password, MFA, login) may be silent for minutes — raise both
limits.

#### Interactive input and secrets

Passwords and yes/no prompts use an **in-app modal** — never put secrets in
**`command`**, **`stdin`**, or tool args.

- For **`ssh`** / **`scp`** / **`sftp`** / **`ssh-copy-id`**: run via
  **`terminal`** and wait; raise **`timeoutMs`** / **`maxWallMs`** for slow
  entry. Do not ask the user to run these in their own terminal.
- Prefer keys when they already work; use **`BatchMode=yes`** only to verify
  key-only login.
- **`stdin`**: non-sensitive one-shot at spawn only (e.g. `y`).
- **`agentRetryForbidden`**: do not retry with secrets; user finishes the modal.
- **`needsInputLikely`**: retry with non-interactive flags or non-sensitive
  **`stdin`** only.

#### Host stop

- **`cancelled`** — the turn was stopped; subprocess force-terminated.
- **`runAborted`** — only this command was stopped; the turn continues.

#### Parameters

| Parameter | Required | Notes |
|-----------|----------|-------|
| **`command`** | yes | Shell command to run |
| **`cwd`** | no | Existing directory |
| **`timeoutMs`** | no | See **Timeouts** |
| **`maxWallMs`** | no | See **Timeouts** |
| **`maxOutputBytes`** | no | Per stream; default and ceiling from host settings
  (default **16 KiB**). Tool arg can only **lower**. Oversize streams keep the
  **tail** (prefix `...[output truncated]`) |
| **`stdin`** | no | Non-sensitive one-shot stdin at spawn |
| **`waitForInputMs`** | no | Max wait for in-app input after prompt detect |
| **`envFiles`** | no | `.env` path or array |
| **`elevated`** | no | Admin/root; see **Elevation** |
| **`label`** | no | Human-readable short name shown in UI (e.g. "运行单元测试"); defaults to first line of `command` |
