### `file`

Unified workspace file tools. Prefer **qualified names** in JSON **`tool_name`**: **`file:read`**, **`file:write`**, **`file:edit`**, **`file:glob`**, **`file:grep`**, **`file:list`** — the runtime merges them into `tool_name` **`file`** plus **`method`**. You may also call **`file`** with a top-level **`method`** string (e.g. **`read`**), equivalent to **`file:read`**, **`file:write`**, etc.

**Relative paths** resolve under the workspace root (`workspaceRoot` in settings, or the process working directory). Do not use `..` to escape the workspace on relative paths. **`file:write`** and **`file:edit`** accept **workspace-relative** paths **or** **absolute** paths that resolve **under the same workspace root** (prefix check after canonicalization); paths outside the workspace are rejected. These writes may require user approval. For **read-only** methods (**`file:read`**, **`file:glob`**, **`file:grep`**, **`file:list`**), you may also use **absolute** paths **outside** the workspace when the user asks.

**Responses:** Whenever this tool returns a filesystem location (`path`, **`matches`**, **`root`**, **`directory`**, grep hit **`path`**, list entry **`path`**), the value is an **absolute** path. The OS may use a canonical form (e.g. resolved symlinks; on Windows, a `\\?\` prefix is normal).

**Batch reads:** If you already know **two or more** file paths, use **`file:read`** with **`paths`** (array) in one call — not multiple reads with **`path`**. Each element may be a **string path** (shared root defaults below) or an **object** with its own **`path`** plus optional **`lineStart`** / **`lineEnd`** / **`maxBytes`** so regions can differ per file in one batch.

**Context discipline:** Each batch returns **full file bodies** (after per-path or root **`lineStart`** / **`lineEnd`** / **`maxBytes`**). Filling **`paths`** with many large files can **overflow the model context** even when under the hard file count. Prefer **narrow batches** (only files you must see together), use **`file:grep`** first, use **`lineStart`** / **`lineEnd`** on huge files, lower **`maxBytes`** when a snippet is enough, or **split across multiple** **`file:read`** turns. The runtime also enforces a **combined `content` budget** per batch (see **`maxTotalBytes`**).

#### Methods

| Method | Purpose |
|--------|---------|
| **`file:read`** | Read UTF-8 text; single file or batch. With `paths`, response shape includes a `files` array. |
| **`file:write`** | Create or overwrite a file; `path` is workspace-relative **or** absolute under the workspace. |
| **`file:edit`** | Replace one unique substring per file: single file (`path` + `oldString` + `newString`) or batch (`edits` array, max **32** entries); each `path` same rule as **`file:write`**. |
| **`file:glob`** | List files matching a glob under the search root (workspace root or optional `base`). |
| **`file:grep`** | Search file contents with a regex. |
| **`file:list`** | List directory entries; optional recursion, max depth, and file/directory filter. |

#### Parameters

All keys below are **JSON properties** on the root **`tool_args`** object of your assistant JSON envelope (alongside **`method`** when using the merged `file` + `method` form).

- **`method`** — Required in **`tool_args`** unless you used a qualified **`tool_name`** (**`file:read`**, **`file:write`**, **`file:edit`**, **`file:glob`**, **`file:grep`**, **`file:list`**). When split, one of: `read`, `write`, `edit`, `glob`, `grep`, `list` (same six as the **`file:…`** names above).

**`file:read`**

- **`path`** — Path to one file (relative to workspace, or absolute for read-only). Use when reading a single file.
- **`paths`** — Array (max **32** entries per call). Each entry is either a **string** (path only; uses root **`lineStart`** / **`lineEnd`** / **`maxBytes`** defaults) or an **object** with **`path`** (alias **`file`**) and optional **`lineStart`** / **`lineEnd`** / **`maxBytes`** (aliases **`line_start`**, **`line_end`**, **`max_bytes`**) for that entry only—omit a field on the object to inherit the root default for that field. Strings and objects may be **mixed** in one array. Response groups results under `files`, and includes **`maxTotalBytes`**, **`contentBytes`**, and **`batchCapped`** (see below).
- **`lineStart`** — Optional (root default for batch string entries and single **`path`**); 1-based first line to include. Default: start of file. Alias **`line_start`**.
- **`lineEnd`** — Optional (root default); 1-based **exclusive** end line (same convention as typical slice end). Alias **`line_end`**.
- **`maxBytes`** — Optional (root default); max bytes read per file (default **262144**, 256 KiB). Alias **`max_bytes`**.
- **`maxTotalBytes`** — **Batch (`paths`) only.** Cap on the combined UTF-8 length of all returned **`content`** strings in this response. Default **1048576** (1 MiB) when omitted; hard maximum **4194304** (4 MiB). Explicit values are clamped to at least **1** byte. If the cap is hit, the tool may **truncate** the last file that fits (see **`batchTruncated`** on that entry) and/or return **`error`** placeholders for paths not read—check **`batchCapped`** on the root object.

**`file:write`**

- **`path`** — Workspace-relative **or** absolute path under the workspace (runtime checks canonical prefix against workspace root).
- **`content`** — Entire file body as one JSON **string** value. Use normal JSON escaping for quotes (`\"`), backslashes (`\\`), and newlines (`\n`); file bytes are UTF-8 text.

**`file:edit`**

- **`path`** — Workspace-relative **or** absolute path under the workspace, targeting an existing file. Use with **`oldString`** and **`newString`** for a **single-file** edit.
- **`oldString`** — Exact snippet to find and replace; must occur **exactly once** in that file. JSON **string**; the runtime also accepts **`old_string`**.
- **`newString`** — Replacement text as a JSON **string**; same escaping rules as **`oldString`**. Alias **`new_string`**.
- **`edits`** — **Batch mode:** non-empty array (max **32**) of objects. Each object requires **`path`** (alias **`file`**), **`oldString`** / **`old_string`**, **`newString`** / **`new_string`**. Do **not** combine **`edits`** with top-level **`path`** / **`oldString`** / **`newString`** in the same call. Entries are applied **in order**; later entries see disk state after earlier ones (including two patches to the **same** path). Response includes **`files`** (each with **`success`**, **`path`**, and either **`replaced`** or **`error`**), **`successCount`**, **`failureCount`**, and **`batchPartialFailure`** (true if any entry failed). Failed entries do **not** roll back earlier successful writes in the same batch—re-read and fix, or follow up with corrective edits.

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

#### JSON example — `file:read` batch with **per-file** line ranges

Root **`lineStart`** / **`lineEnd`** still apply to **string** entries. **Object** entries may override **`lineStart`**, **`lineEnd`**, and **`maxBytes`** for that path only.

```json
{
  "thoughts": "Read the header of lib.rs and a middle slice of main.rs.",
  "headline": "Batch read with ranges",
  "tool_name": "file:read",
  "tool_args": {
    "lineStart": 1,
    "lineEnd": 40,
    "paths": [
      "crates/foo/src/lib.rs",
      {
        "path": "crates/foo/src/main.rs",
        "lineStart": 80,
        "lineEnd": 120
      }
    ]
  }
}
```

#### JSON example — `file:edit` batch (`edits`)

```json
{
  "thoughts": "Rename symbol in two modules.",
  "headline": "Batch edit",
  "tool_name": "file:edit",
  "tool_args": {
    "edits": [
      {
        "path": "src/a.ts",
        "oldString": "export const OLD = 1",
        "newString": "export const NEW = 1"
      },
      {
        "path": "src/b.ts",
        "oldString": "import { OLD } from './a'",
        "newString": "import { NEW } from './a'"
      }
    ]
  }
}
```
