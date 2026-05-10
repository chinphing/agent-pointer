### `file`

Unified workspace file tools (same category as Computer agent’s `mouse:method`). Prefer **qualified names** in XML: `file:read`, `file:write`, `file:edit`, `file:glob`, `file:grep` — the runtime merges them into `tool_name` `file` plus `method`. You may also call `file` with a top-level **`method`** string.

All paths are relative to the workspace root (`workspaceRoot` in settings, or the process working directory). Do not escape with `..`. **`write`** and **`edit`** may require user approval. **`grep`** uses Rust regex syntax.

**Batch reads:** If you already know **two or more** file paths, use **`file:read`** with **`paths`** (array) in one call — not multiple reads with **`path`**.

#### Methods

| `method` | Purpose |
|----------|---------|
| `read` | Read UTF-8 text; single file or batch. With `paths`, response shape includes a `files` array. |
| `write` | Create or overwrite a file. |
| `edit` | Replace one unique substring in a file. |
| `glob` | List files matching a glob under the workspace root. |
| `grep` | Search file contents with a regex. |

#### Parameters

- **`method`** — Required in tool arguments unless you used a qualified XML name (`file:read`, …). One of: `read`, `write`, `edit`, `glob`, `grep`.

**Read (`read`)**

- **`path`** — Relative path to one file. Use when reading a single file.
- **`paths`** — Array of relative paths (max **32** per call). Prefer this when you already know two or more paths. Response groups results under `files`.
- **`lineStart`** — Optional; 1-based first line to include. Default: start of file.
- **`lineEnd`** — Optional; 1-based **exclusive** end line (same convention as typical slice end).
- **`maxBytes`** — Optional; max bytes read per file (default **524288**).

**Write (`write`)**

- **`path`** — Relative path of the file to create or overwrite.
- **`content`** — Full file body as UTF-8 text. In XML, wrap in **CDATA**.

**Edit (`edit`)**

- **`path`** — Relative path to an existing file.
- **`oldString`** — Exact snippet to replace; must match the file uniquely. Implementations also accept **`old_string`**. In XML, use **CDATA** (handles `<`, `&`, markup).
- **`newString`** — Replacement text. Also accepts **`new_string`**. XML: **CDATA**.

**Glob (`glob`)**

- **`pattern`** — Glob pattern (e.g. `**/*.rs`). Matched against paths relative to the workspace root.
- **`maxResults`** — Optional cap on returned paths (default bounded by runtime, max **500**).
- **`maxDepth`** — Optional directory walk depth cap (default **64**).

**Grep (`grep`)**

- **`pattern`** — Rust regex (multi-line). Keep patterns reasonably short (e.g. ≤ **512** characters).
- **`subdir`** — Optional; restrict search to this subdirectory under the workspace root (relative path).
- **`maxResults`** — Optional cap on hit rows (default bounded by runtime).
- **`maxDepth`** — Optional directory walk depth cap.
- **`contextLines`** — Optional lines of context above/below each match (default **2**, clamped up to **5**).
