---
schema:
  type: object
  properties:
    method:
      type: string
      enum:
        - read
        - write
        - edit
        - glob
        - grep
        - list
    path:
      type: string
    paths:
      type: array
    content: {}
    edits:
      type: array
    pattern:
      type: string
  additionalProperties: true
---

### `file`

Unified workspace file tools. Prefer **qualified names** in JSON **`tool_name`**: **`file:read`**, **`file:write`**, **`file:edit`**, **`file:glob`**, **`file:grep`**, **`file:list`** — the runtime merges them into `tool_name` **`file`** plus **`method`**. You may also call **`file`** with a top-level **`method`** string (e.g. **`read`**), equivalent to **`file:read`**, **`file:write`**, etc.

**Relative paths** resolve under the workspace root (`workspaceRoot` in settings, or the process working directory). Do not use `..` to escape the workspace on relative paths. **`file:write`** and **`file:edit`** accept **workspace-relative** paths **or** **absolute** paths that resolve **under the same workspace root** (prefix check after canonicalization); paths outside the workspace are rejected. These writes may require user approval. For **read-only** methods (**`file:read`**, **`file:glob`**, **`file:grep`**, **`file:list`**), you may also use **absolute** paths **outside** the workspace when the user asks.

**Responses:** Whenever this tool returns a filesystem location (`path`, **`matches`**, **`root`**, **`directory`**, grep hit **`path`**, list entry **`path`**), the value is an **absolute** path. The OS may use a canonical form (e.g. resolved symlinks; on Windows, a `\\?\` prefix is normal).

**Batch reads:** If you already know **two or more** file paths, use **`file:read`** with **`paths`** (JSON array) in one call — not multiple reads with **`path`**. **Each array element must be an object** with required **`path`** (alias **`file`**) and optional **`lineStart`** / **`lineEnd`** / **`maxBytes`** (aliases **`line_start`**, **`line_end`**, **`max_bytes`**). Omitting a field on the object uses the root-level default for that field (see below). Do **not** use bare string paths as **`paths`** elements — the runtime rejects them.

**Context discipline:** Each batch returns **full file bodies** (after per-path or root **`lineStart`** / **`lineEnd`** / **`maxBytes`**). Filling **`paths`** with many large files can **overflow the model context** even when under the hard file count. Prefer **narrow batches** (only files you must see together), use **`file:grep`** first, use **`lineStart`** / **`lineEnd`** on huge files, lower **`maxBytes`** when a snippet is enough, or **split across multiple** **`file:read`** turns. The runtime also enforces a **combined `content` budget** per batch (see **`maxTotalBytes`**).

#### Methods

| Method | Purpose |
|--------|---------|
| **`file:read`** | Read UTF-8 text; single file or batch. With `paths`, response shape includes a `files` array. |
| **`file:write`** | Create or overwrite a file; `path` is workspace-relative **or** absolute under the workspace. |
| **`file:edit`** | Replace one unique substring per file via **`edits`** only: a non-empty array (max **32**) of objects, each with **`path`** (alias **`file`**), **`oldString`** / **`old_string`**, **`newString`** / **`new_string`**. Single-file edits use **`edits`** with **one** object. Response includes **`files`**, **`successCount`**, **`failureCount`**, **`batchPartialFailure`**. |
| **`file:glob`** | List paths matching a glob under the search root (workspace root or optional `base`). Default: **files only**; optional **directories** or **both**. |
| **`file:grep`** | Search file contents with a regex (ripgrep-class stack: respects `.gitignore`, skips hidden paths by default, line-oriented matching). |
| **`file:list`** | List directory entries; optional recursion, max depth, and file/directory filter. |

#### Parameters

All keys below are **JSON properties** on the root **`tool_args`** object of your assistant JSON envelope (alongside **`method`** when using the merged `file` + `method` form).

- **`method`** — Required in **`tool_args`** unless you used a qualified **`tool_name`** (**`file:read`**, **`file:write`**, **`file:edit`**, **`file:glob`**, **`file:grep`**, **`file:list`**). When split, one of: `read`, `write`, `edit`, `glob`, `grep`, `list` (same six as the **`file:…`** names above).

**`file:read`**

- **`path`** — Path to one file (relative to workspace, or absolute for read-only). Use when reading a single file.
- **`paths`** — Array (max **32** entries per call). **Each entry is an object** with **`path`** (alias **`file`**) and optional **`lineStart`** / **`lineEnd`** / **`maxBytes`** (aliases **`line_start`**, **`line_end`**, **`max_bytes`**). Omit a field on the object to use the root default for that field. Response groups results under `files`, and includes **`maxTotalBytes`**, **`contentBytes`**, and **`batchCapped`** (see below).
- **`lineStart`** — Optional (root default for batch string entries and single **`path`**); 1-based first line to include. Default: start of file. Alias **`line_start`**.
- **`lineEnd`** — Optional (root default); 1-based **exclusive** end line (same convention as typical slice end). Alias **`line_end`**.
- **`maxBytes`** — Optional (root default); max bytes read per file (default **262144**, 256 KiB). Alias **`max_bytes`**.
- **`maxTotalBytes`** — **Batch (`paths`) only.** Cap on the combined UTF-8 length of all returned **`content`** strings in this response. Default **1048576** (1 MiB) when omitted; hard maximum **4194304** (4 MiB). Explicit values are clamped to at least **1** byte. If the cap is hit, the tool may **truncate** the last file that fits (see **`batchTruncated`** on that entry) and/or return **`error`** placeholders for paths not read—check **`batchCapped`** on the root object.

**`file:write`**

- **`path`** — Workspace-relative **or** absolute path under the workspace (runtime checks canonical prefix against workspace root).
- **`content`** — Entire file body. Prefer a JSON **string** (use `\"`, `\\`, `\n` as needed). You may also pass a JSON **object** or **array**; the runtime pretty-prints it as UTF-8 text (typical for `.json` / config files).

**`file:edit`**

- **`edits`** — **Required.** Non-empty array (max **32**) of objects. Each object requires **`path`** (alias **`file`**), **`oldString`** / **`old_string`**, **`newString`** / **`new_string`**. For a **single-file** edit, pass **one** element. Entries are applied **in order**; later entries see disk state after earlier ones (including two patches to the **same** path). Response includes **`files`** (each with **`success`**, **`path`**, and either **`replaced`** or **`error`**), **`successCount`**, **`failureCount`**, and **`batchPartialFailure`** (true if any entry failed). Failed entries do **not** roll back earlier successful writes in the same batch—re-read and fix, or follow up with corrective edits. Do **not** pass top-level **`path`** / **`oldString`** / **`newString`** alongside **`edits`**.

**`file:glob`**

- **`pattern`** — Glob pattern (e.g. `**/*.rs`). Matched against paths relative to the search root (use `/` in patterns; the tool normalizes OS separators).
- **`base`** — Optional; alias **`rootPath`** / **`baseDir`**. Directory to search under (relative to workspace or absolute for read-only). Default: workspace root.
- **`maxResults`** — Optional cap on returned paths (default bounded by runtime, max **500**).
- **`maxDepth`** — Optional directory walk depth cap (default **64**).
- **`entryType`** — Optional; alias **`entry_type`**. One of **`file`** / **`files`** (default), **`dir`** / **`directory`** / **`directories`**, or **`all`** — return only files, only directories, or both.
- **`includeHidden`** — Optional boolean. When **`false`** (default), the walk skips entries whose **basename** starts with **`.`** (except the search root). Set **`true`** to include those paths (e.g. to match a top-level **`.git`** directory with **`entryType`** **`dir`**). Descending **inside** a repository metadata tree under **`.git/`** is skipped for cost; the **`.git`** directory itself can still match when allowed by **`entryType`** and **`includeHidden`**.

**`file:grep`**

- **`pattern`** — Rust regex syntax (via the same matcher stack ripgrep uses for line search). Keep patterns reasonably short (e.g. ≤ **512** characters). Matching is **line-oriented** (not multi-line across `\\n` within one match). When **`fixedString`** is `true`, `pattern` is treated as a literal string, not a regex.
- **`path`** — Optional; same idea as **`grep -R pattern PATH`**: **`PATH`** may be a **file** (search that file only) or a **directory** (walk files under it). Omit or use an empty string to search from the **workspace root**. Workspace-relative or absolute read-only. Directory walks honor **`.gitignore`** / ignore rules and **skip hidden** entries by default (like ripgrep).
- **`maxResults`** — Optional cap on hit rows (default bounded by runtime).
- **`maxDepth`** — Optional directory walk depth cap (ignored when **`path`** targets a single file).
- **`contextLines`** — Optional lines of context above/below each match (default **2**, clamped up to **5**).
- **`includeGlobs`** — Optional array of include glob patterns (e.g. `["*.rs", "src/**/*"]`). Only files matching any pattern are searched. When combined with **`fileTypes`**, those globs are merged (duplicates removed).
- **`excludeGlobs`** — Optional array of exclude glob patterns. Files matching any pattern are skipped. Exclude takes priority over include.
- **`fileTypes`** — Optional array of predefined type names (e.g. `["rust", "js"]`). Each name expands to a set of include globs: `"rust"` → `["*.rs", "*.toml"]`; `"py"` → `["*.py", "*.pyi"]`; `"js"` → `["*.js", "*.cjs", "*.mjs"]`; `"ts"` → `["*.ts", "*.tsx"]`; `"vue"` → `["*.vue"]`; `"md"` → `["*.md"]`; `"json"` → `["*.json"]`. Invalid type names return an error.
- **`fixedString`** — Optional boolean; when `true` the search is literal (no regex). Default `false`.
- **`ignoreCase`** — Optional boolean; when `true` case-insensitive matching is enabled. Default `false`.
- **`includeHidden`** — Optional boolean; when `true` hidden files and directories are included in the walk (overrides the default skip-hidden behavior). Default `false`.

Binary files are skipped heuristically (NUL byte). Very large files (&gt; **2 MiB**) are skipped per file, same budget idea as before.

Response includes **`singleFile`: true`** when **`path`** resolves to a **file**.

**`file:list`**

- **`path`** — Required; directory to list (alias **`directory`**). Relative to workspace or absolute.
- **`recursive`** — Optional boolean; default **false** (immediate children only). When **true**, walk subdirectories.
- **`maxDepth`** — Optional; when **`recursive`** is true, max WalkDir depth from the listed directory (default **8**, capped by runtime). Ignored for non-recursive listing.
- **`entryType`** — Optional; alias **`entry_type`**. One of **`all`** (default), **`file`** / **`files`**, **`dir`** / **`directory`** / **`directories`** — return only files, only directories, or both.
