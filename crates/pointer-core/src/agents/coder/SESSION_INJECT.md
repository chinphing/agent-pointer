## Session context (runtime)

The host expands placeholders in this file **once per chat request** and inserts the result into the system prompt (after environment, before the agent manifest body).

**Workspace root** (absolute path from app settings): `{{workspace_root}}`

When this path is non-empty, relative paths for the workspace **`file`** tool (`file:read`, `file:write`, `file:edit`, `file:glob`, `file:grep`), and the default working directory for `terminal`, are resolved under this root. When empty, tools follow the application’s default resolution (e.g. process current directory).
