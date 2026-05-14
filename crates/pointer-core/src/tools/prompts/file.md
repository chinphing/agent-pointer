### `file`

Unified workspace file tools. Prefer **qualified names** in JSON **`tool_name`**: **`file:read`**, **`file:write`**, **`file:edit`**, **`file:glob`**, **`file:grep`**, **`file:list`** — the runtime merges them into `tool_name` **`file`** plus **`method`**. You may also call **`file`** with a top-level **`method`** string (e.g. **`read`**), equivalent to **`file:read`**, **`file:write`**, etc.

**Relative paths** resolve under the workspace root (`workspaceRoot` in settings, or the process working directory). Do not use `..` to escape the workspace on relative paths. **`file:write`** and **`file:edit`** only accept workspace-relative paths and may require user approval. For **read-only** methods (**`file:read`**, **`file:glob`**, **`file:grep`**, **`file:list`**), you may use **absolute** paths to inspect another project when the user asks.

**Batch reads:** If you already know **two or more** file paths, use **`file:read`** with **`paths`** (array) in one call — not multiple reads with **`path`**.

**Context discipline:** Each batch returns **full file bodies** (after **`lineStart`** / **`lineEnd`** / **`maxBytes`**). Filling **`paths`** with many large files can **overflow the model context** even when under the hard file count. Prefer **narrow batches** (only files you must see together), use **`file:grep`** first, use **`lineStart`** / **`lineEnd`** on huge files, lower **`maxBytes`** when a snippet is enough, or **split across multiple** **`file:read`** turns. The runtime also enforces a **combined `content` budget** per batch (see **`maxTotalBytes`**).

#### Methods

| Method | Purpose |
|--------|---------|
| **`file:read`** | Read UTF-8 text; single file or batch. With `paths`, response shape includes a `files` array. |
| **`file:write`** | Create or overwrite a file (workspace-relative `path` only). |
| **`file:edit`** | Replace one unique substring in a file (workspace-relative `path` only). |
| **`file:glob`** | List files matching a glob under the search root (workspace root or optional `base`). |
| **`file:grep`** | Search file contents with a regex. |
| **`file:list`** | List directory entries; optional recursion, max depth, and file/directory filter. |

#### Parameters

All keys below are **JSON properties** on the root **`tool_args`** object of your assistant JSON envelope (alongside **`method`** when using the merged `file` + `method` form).

- **`method`** — Required in **`tool_args`** unless you used a qualified **`tool_name`** (**`file:read`**, **`file:write`**, **`file:edit`**, **`file:glob`**, **`file:grep`**, **`file:list`**). When split, one of: `read`, `write`, `edit`, `glob`, `grep`, `list` (same six as the **`file:…`** names above).

**`file:read`**

- **`path`** — Path to one file (relative to workspace, or absolute for read-only). Use when reading a single file.
- **`paths`** — Array of paths (max **32** per call). Prefer when you already know two or more paths. Response groups results under `files`, and includes **`maxTotalBytes`**, **`contentBytes`**, and **`batchCapped`** (see below).
- **`lineStart`** — Optional; 1-based first line to include. Default: start of file.
- **`lineEnd`** — Optional; 1-based **exclusive** end line (same convention as typical slice end).
- **`maxBytes`** — Optional; max bytes read per file (default **262144**, 256 KiB).
- **`maxTotalBytes`** — **Batch (`paths`) only.** Cap on the combined UTF-8 length of all returned **`content`** strings in this response. Default **1048576** (1 MiB) when omitted; hard maximum **4194304** (4 MiB). Explicit values are clamped to at least **1** byte. If the cap is hit, the tool may **truncate** the last file that fits (see **`batchTruncated`** on that entry) and/or return **`error`** placeholders for paths not read—check **`batchCapped`** on the root object.

**`file:write`**

- **`path`** — Relative path of the file to create or overwrite (workspace only).
- **`content`** — Entire file body as one JSON **string** value. Use normal JSON escaping for quotes (`\"`), backslashes (`\\`), and newlines (`\n`); file bytes are UTF-8 text.

**`file:edit`**

- **`path`** — Relative path to an existing file (workspace only).
- **`oldString`** — Exact snippet to find and replace; must occur **exactly once** in the file. JSON **string** in **`tool_args`**; any `<`, `>`, `&`, or markup are literal characters inside that string—only JSON’s own escaping rules apply. The runtime also accepts the alias **`old_string`**.
- **`newString`** — Replacement text as a JSON **string** in **`tool_args`**; same escaping rules as **`oldString`**. Alias **`new_string`** is accepted.

**`file:glob`**

- **`pattern`** — Glob pattern (e.g. `**/*.rs`). Matched against paths relative to the search root.
- **`base`** — Optional; alias **`rootPath`** / **`baseDir`**. Directory to search under (relative to workspace or absolute for read-only). Default: workspace root.
- **`maxResults`** — Optional cap on returned paths (default bounded by runtime, max **500**).
- **`maxDepth`** — Optional directory walk depth cap (default **64**).

**`file:grep`**

- **`pattern`** — Rust regex (multi-line). Keep patterns reasonably short (e.g. ≤ **512** characters).
- **`path`** — Optional; same idea as **`grep -R pattern PATH`**: **`PATH`** may be a **file** (search that file only) or a **directory** (walk files under it). Omit or use an empty string to search from the **workspace root**. Workspace-relative or absolute read-only.
- **`maxResults`** — Optional cap on hit rows (default bounded by runtime).
- **`maxDepth`** — Optional directory walk depth cap (ignored when **`path`** targets a single file).
- **`contextLines`** — Optional lines of context above/below each match (default **2**, clamped up to **5**).

Response includes **`singleFile`: true** when **`path`** resolves to a **file**.

**`file:list`**

- **`path`** — Required; directory to list (alias **`directory`**). Relative to workspace or absolute.
- **`recursive`** — Optional boolean; default **false** (immediate children only). When **true**, walk subdirectories.
- **`maxDepth`** — Optional; when **`recursive`** is true, max WalkDir depth from the listed directory (default **8**, capped by runtime). Ignored for non-recursive listing.
- **`entryType`** — Optional; alias **`entry_type`**. One of **`all`** (default), **`file`** / **`files`**, **`dir`** / **`directory`** / **`directories`** — return only files, only directories, or both.

#### JSON example — `file:read` batch

Use a real JSON array for **`paths`** inside **`tool_args`**.

```json
{
  "thoughts": "Read implementation and tests together.",
  "headline": "Batch read",
  "tool_name": "file:read",
  "tool_args": {
    "paths": ["crates/foo/src/lib.rs", "crates/foo/src/main.rs"],
    "lineStart": 1
  }
}
```
