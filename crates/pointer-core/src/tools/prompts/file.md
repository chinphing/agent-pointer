---
schema:
  type: object
  properties:
    paths:
      type: array
    path:
      type: string
    content: {}
    edits:
      type: array
    pattern:
      type: string
  additionalProperties: true
---

### `file`

Independent file tools. Call them directly by their flat names:

- **`file_read`** — read UTF-8 text files.
- **`file_write`** — create or overwrite a file.
- **`file_edit`** — replace unique substrings in files.
- **`file_glob`** — list paths matching a glob pattern.
- **`file_grep`** — search file contents with a regex.
- **`file_list`** — list directory entries.

**Relative paths** resolve under the workspace root (`workspaceRoot` in settings, or the process working directory). Do not use `..` to escape the workspace on relative paths. **`file_write`** and **`file_edit`** accept **workspace-relative** paths **or** **absolute** paths that resolve **under the same workspace root** (prefix check after canonicalization); paths outside the workspace are rejected. These writes may require user approval. For **read-only** methods (**`file_read`**, **`file_glob`**, **`file_grep`**, **`file_list`**), you may also use **absolute** paths **outside** the workspace when the user asks.

**Responses:** Whenever this tool returns a filesystem location (`path`, **`matches`**, **`root`**, **`directory`**, grep hit **`path`**, list entry **`path`**), the value is an **absolute** path. The OS may use a canonical form (e.g. resolved symlinks; on Windows, a `\\?\` prefix is normal).

**Reading:** Always use **`read`** with **`paths`** — a JSON array of objects, **even for a single file**. Do **not** pass a top-level **`path`** on **`read`**. **Each array element must be an object** with required **`path`** (alias **`file`**) and optional **`lineStart`** / **`lineEnd`** / **`maxBytes`** (aliases **`line_start`**, **`line_end`**, **`max_bytes`**). Omitting a field on the object uses the root-level default for that field. Do **not** use bare string paths as **`paths`** elements — the runtime rejects them.

**Context discipline:** Each read returns **full file bodies** (after per-path or root **`lineStart`** / **`lineEnd`** / **`maxBytes`**). Filling **`paths`** with many large files can **overflow the model context** even when under the hard file count. Prefer **narrow batches** (only files you must see together), use **`grep`** first, use **`lineStart`** / **`lineEnd`** on huge files, lower **`maxBytes`** when a snippet is enough, or **split across multiple** **`read`** turns. The runtime also enforces a **combined `content` budget** per batch (see **`maxTotalBytes`**).

#### Methods

| Method | Purpose |
|--------|---------|
| **`read`** | Read UTF-8 text via **`paths`** array. Response includes a **`files`** array. |
| **`write`** | Create or overwrite a file; `path` is workspace-relative **or** absolute under the workspace. |
| **`edit`** | Replace one unique substring per file via **`edits`** only: a non-empty array (max **32**) of objects, each with **`path`** (alias **`file`**), **`oldString`** / **`old_string`**, **`newString`** / **`new_string`**. Single-file edits use **`edits`** with **one** object. Response includes **`files`**, **`successCount`**, **`failureCount`**, **`batchPartialFailure`**. |
| **`glob`** | List paths matching a glob under the search root (workspace root or optional `base`). Default: **files only**; optional **directories** or **both**. |
| **`grep`** | Search file contents with a regex (ripgrep-class stack: respects `.gitignore`, skips hidden paths by default, line-oriented matching). |
| **`list`** | List directory entries; **recursive by default** (depth 2); optional maxResults cap and file/directory filter. |

#### Parameters

**`read`**

- **`paths`** — **Required.** Array (max **32** entries per call). **Each entry is an object** with **`path`** (alias **`file`**) and optional **`lineStart`** / **`lineEnd`** / **`maxBytes`**. Single file: **`paths: [{ "path": "src/foo.rs" }]`**. Response groups results under **`files`**, and includes **`maxTotalBytes`**, **`contentBytes`**, and **`batchCapped`**.
  **Windows paths:** escape backslashes in JSON — `\\` for each `\`. Example: `"D:\\workspace\\src\\foo.rs"`, not `"D:\workspace\src\foo.rs"`.
- **`lineStart`** — Optional root default for batch entries; 1-based first line to include. Default: start of file. Alias **`line_start`**.
- **`lineEnd`** — Optional root default; 1-based **exclusive** end line. Alias **`line_end`**.
- **`maxBytes`** — Optional root default; max bytes read per file (default **262144**, 256 KiB). Alias **`max_bytes`**.
- **`maxTotalBytes`** — Cap on combined UTF-8 length of all returned **`content`** strings. Default **1048576** (1 MiB); hard maximum **4194304** (4 MiB).

Example — single file:

```json
{
  "function": {
    "name": "file_read",
    "arguments": {
      "paths": [{ "path": "src/foo.rs" }]
    }
  }
}
```

Example — batch with line ranges (Windows):

```json
{
  "function": {
    "name": "file_read",
    "arguments": {
      "paths": [
        { "path": "D:\\workspace\\src\\a.rs", "lineStart": 10, "lineEnd": 80 },
        { "path": "D:\\workspace\\src\\b.rs" }
      ],
      "maxTotalBytes": 524288
    }
  }
}
```

**`write`**

- **`path`** — Workspace-relative **or** absolute path under the workspace. Windows: escape `\\` in JSON (same rule as `paths` above).
- **`content`** — Entire file body. Prefer a JSON **string** (use `\"`, `\\`, `\n` as needed). You may also pass a JSON **object** or **array**; the runtime pretty-prints it as UTF-8 text.

Example:

```json
{
  "function": {
    "name": "file_write",
    "arguments": {
      "path": "src/components/Foo.vue",
      "content": "<template><div>Hello</div></template>\n"
    }
  }
}
```

**`edit`**

- **`edits`** — **Required.** Non-empty array (max **32**) of objects. Each object requires **`path`**, **`oldString`** / **`old_string`**, **`newString`** / **`new_string`**. For a **single-file** edit, pass **one** element. Do **not** pass top-level **`path`** / **`oldString`** / **`newString`** alongside **`edits`**.

Example:

```json
{
  "function": {
    "name": "file_edit",
    "arguments": {
      "edits": [
        {
          "path": "src/App.vue",
          "oldString": "  <div v-if=\"x\">before</div>",
          "newString": "  <div v-if=\"x\">after</div>"
        }
      ]
    }
  }
}
```

**`glob`**

- **`pattern`** — Glob pattern (e.g. `**/*.rs`). Matched against paths relative to the search root.
- **`base`** — Optional; alias **`rootPath`** / **`baseDir`**. Directory to search under. Default: workspace root.
- **`maxResults`** — Optional cap (default bounded by runtime, max **500**).
- **`maxDepth`** — Optional directory walk depth cap (default **64**).
- **`entryType`** — Optional; alias **`entry_type`**. **`file`** (default), **`dir`**, or **`all`**.
- **`includeHidden`** — Optional boolean (default **`false`**).

**`grep`**

- **`pattern`** — Rust regex syntax (via the same matcher stack ripgrep uses for line search). Keep patterns reasonably short (≤ **512** characters). Matching is **line-oriented** (not multi-line across `\n` within one match). When **`fixedString`** is `true`, `pattern` is treated as a literal string, not a regex.
- **`path`** — Optional; same idea as **`grep -R pattern PATH`**: **`PATH`** must be an **existing** file or directory. Omit or use an empty string to search from the **workspace root**. Prefer **workspace-relative** paths (e.g. `src/`). If the path does not exist, the error includes **可能的路径** — sibling directories under the nearest existing parent (or workspace root) to help correct typos like `ui` → `src`.
- **`maxResults`** — Optional cap on hit rows (default bounded by runtime).
- **`maxDepth`** — Optional directory walk depth cap (ignored when **`path`** targets a single file).
- **`contextLines`** — Optional lines of context above/below each match (default **2**, clamped up to **5**). Alias **`context_lines`**.
- **`includeGlobs`** — Optional array of include glob patterns (e.g. `["*.rs", "src/**/*"]`). Only files matching any pattern are searched. When combined with **`fileTypes`**, those globs are merged (duplicates removed).
- **`excludeGlobs`** — Optional array of exclude glob patterns. Files matching any pattern are skipped. Exclude takes priority over include.
- **`fileTypes`** — Optional array of predefined type names (e.g. `["rust", "js"]`). Each name expands to include globs; aliases and case are normalized. Invalid names return an error. Supported types:

| Type | Aliases | Globs |
|------|---------|-------|
| `rust` | `rs` | `*.rs` |
| `toml` | — | `*.toml` |
| `py` | `python` | `*.py`, `*.pyi` |
| `js` | `javascript` | `*.js`, `*.jsx`, `*.cjs`, `*.mjs` |
| `ts` | `typescript` | `*.ts`, `*.tsx`, `*.mts`, `*.cts` |
| `vue` | — | `*.vue` |
| `svelte` | — | `*.svelte` |
| `tsx` | — | `*.tsx` |
| `md` | `markdown` | `*.md`, `*.mdx` |
| `json` | — | `*.json`, `*.jsonc` |
| `yaml` | `yml` | `*.yml`, `*.yaml` |
| `html` | — | `*.html`, `*.htm` |
| `css` | — | `*.css`, `*.scss`, `*.less` |
| `xml` | — | `*.xml`, `*.xsl`, `*.xslt` |
| `go` | — | `*.go` |
| `java` | — | `*.java` |
| `kt` | `kotlin` | `*.kt`, `*.kts` |
| `c` | — | `*.c`, `*.h` |
| `cpp` | `c++`, `cxx`, `hpp` | `*.cpp`, `*.cc`, `*.cxx`, `*.hpp`, `*.hh`, `*.hxx` |
| `rb` | `ruby` | `*.rb`, `*.rake`, `*.gemspec` |
| `php` | — | `*.php`, `*.phtml` |
| `swift` | — | `*.swift` |
| `scala` | — | `*.scala`, `*.sc` |
| `sql` | — | `*.sql` |
| `sh` | `shell`, `bash`, `zsh` | `*.sh`, `*.bash`, `*.zsh` |
- **`fixedString`** — Optional boolean; when `true` the search is literal (no regex). Default `false`.
- **`ignoreCase`** — Optional boolean; when `true` case-insensitive matching is enabled. Default `false`.
- **`includeHidden`** — Optional boolean; when `true` hidden files and directories are included in the walk (overrides the default skip-hidden behavior). Default `false`.

Binary files are skipped heuristically (NUL byte). Very large files (> **2 MiB**) are skipped per file.

Response includes **`singleFile`: true** when **`path`** resolves to a **file**.

**`list`**

- **`path`** — Required; directory to list (alias **`directory`**).
- **`recursive`** — Optional boolean; default **true**.
- **`maxDepth`** — Optional when **`recursive`** is true (default **2**).
- **`maxResults`** — Optional cap on returned entries (default **100**, max **2000**).
- **`entryType`** — Optional; **`all`** (default), **`file`**, or **`dir`**.
