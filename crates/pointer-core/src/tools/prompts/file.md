### `file`

Unified workspace file tools (same category as Computer agent’s `mouse:method`). Prefer **qualified names** in XML: `file:read`, `file:write`, `file:edit`, `file:glob`, `file:grep`, `file:list` — the runtime merges them into `tool_name` `file` plus `method`. You may also call `file` with a top-level **`method`** string.

**Relative paths** resolve under the workspace root (`workspaceRoot` in settings, or the process working directory). Do not use `..` to escape the workspace on relative paths. **`write`** and **`edit`** only accept workspace-relative paths and may require user approval. For **read-only** methods (`read`, `glob`, `grep`, `list`), you may use **absolute** paths to inspect another project when the user asks.

**Batch reads:** If you already know **two or more** file paths, use **`file:read`** with **`paths`** (array) in one call — not multiple reads with **`path`**.

#### Methods

| `method` | Purpose |
|----------|---------|
| `read` | Read UTF-8 text; single file or batch. With `paths`, response shape includes a `files` array. |
| `write` | Create or overwrite a file (workspace-relative `path` only). |
| `edit` | Replace one unique substring in a file (workspace-relative `path` only). |
| `glob` | List files matching a glob under the search root (workspace root or optional `base`). |
| `grep` | Search file contents with a regex. |
| `list` | List directory entries; optional recursion, max depth, and file/directory filter. |

#### Parameters

- **`method`** — Required in tool arguments unless you used a qualified XML name (`file:read`, …). One of: `read`, `write`, `edit`, `glob`, `grep`, `list`.

**Read (`read`)**

- **`path`** — Path to one file (relative to workspace, or absolute for read-only). Use when reading a single file.
- **`paths`** — Array of paths (max **32** per call). Prefer when you already know two or more paths. Response groups results under `files`.
- **`lineStart`** — Optional; 1-based first line to include. Default: start of file.
- **`lineEnd`** — Optional; 1-based **exclusive** end line (same convention as typical slice end).
- **`maxBytes`** — Optional; max bytes read per file (default **524288**).

**Write (`write`)**

- **`path`** — Relative path of the file to create or overwrite (workspace only).
- **`content`** — Full file body as UTF-8 text. In XML, wrap in **CDATA**.

**Edit (`edit`)**

- **`path`** — Relative path to an existing file (workspace only).
- **`oldString`** — Exact snippet to replace; must match the file uniquely. Implementations also accept **`old_string`**. In XML, use **CDATA** (handles `<`, `&`, markup).
- **`newString`** — Replacement text. Also accepts **`new_string`**. XML: **CDATA**.

**Glob (`glob`)**

- **`pattern`** — Glob pattern (e.g. `**/*.rs`). Matched against paths relative to the search root.
- **`base`** — Optional; alias **`rootPath`** / **`baseDir`**. Directory to search under (relative to workspace or absolute for read-only). Default: workspace root.
- **`maxResults`** — Optional cap on returned paths (default bounded by runtime, max **500**).
- **`maxDepth`** — Optional directory walk depth cap (default **64**).

**Grep (`grep`)**

- **`pattern`** — Rust regex (multi-line). Keep patterns reasonably short (e.g. ≤ **512** characters).
- **`subdir`** — Optional; restrict search to this directory (relative to workspace or absolute path to an existing directory).
- **`maxResults`** — Optional cap on hit rows (default bounded by runtime).
- **`maxDepth`** — Optional directory walk depth cap.
- **`contextLines`** — Optional lines of context above/below each match (default **2**, clamped up to **5**).

**List (`list`)**

- **`path`** — Required; directory to list (alias **`directory`**). Relative to workspace or absolute.
- **`recursive`** — Optional boolean; default **false** (immediate children only). When **true**, walk subdirectories.
- **`maxDepth`** — Optional; when **`recursive`** is true, max WalkDir depth from the listed directory (default **8**, capped by runtime). Ignored for non-recursive listing.
- **`entryType`** — Optional; alias **`entry_type`**. One of **`all`** (default), **`file`** / **`files`**, **`dir`** / **`directory`** / **`directories`** — return only files, only directories, or both.

#### XML example — `file:read` batch

Put a **JSON array string** inside `<paths>` so it parses as an array (not multiple `<path>` tags).

```xml
<response>
  <thoughts>Read implementation and tests together.</thoughts>
  <headline>Batch read</headline>
  <tool_name>file:read</tool_name>
  <tool_args>
    <paths>["crates/foo/src/lib.rs","crates/foo/src/main.rs"]</paths>
    <lineStart>1</lineStart>
  </tool_args>
</response>
```
