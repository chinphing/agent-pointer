## Session context (runtime)

The host expands placeholders in this file **once per chat request** and inserts the result into the system prompt (after environment, before the agent manifest body).

**Workspace root** (absolute path from app settings): `{{workspace_root}}`

When this path is non-empty, **relative** paths for the **`file`** tool (`file:read`, `file:write`, `file:edit`, `file:glob`, `file:grep`, `file:list`), and the default working directory for `terminal`, are resolved under this root. **Absolute** paths are accepted for **read-only** `file` methods (`read`, `glob`, `grep`, `list`) so you can inspect code the user points to outside this folder; **`write`** / **`edit`** still use workspace-relative paths only. When empty, relative paths follow the application’s default resolution (e.g. process current directory).
