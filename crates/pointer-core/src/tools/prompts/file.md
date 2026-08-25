---
schema:
  type: object
  properties:
    path:
      type: string
    content: {}
    oldString:
      type: string
    newString:
      type: string
    pattern:
      type: string
  additionalProperties: true
---

### `file`

Independent file tools. Call them directly by their flat names:

- **`file_read`** — read one UTF-8 text file per call.
- **`file_write`** — create or overwrite a file.
- **`file_edit`** — replace one unique substring in one file per call.
- **`file_glob`** — list paths matching a glob pattern.
- **`file_grep`** — search file contents with a regex.
- **`file_list`** — list directory entries.

**Relative paths** resolve under the workspace root (`workspaceRoot` in settings, or the process working directory). Do not use `..` to escape the workspace on relative paths. **`file_write`** and **`file_edit`** accept **workspace-relative** paths **or** **absolute** paths (including **`~`**) that resolve **under an allowed write root**: workspace, user home, system temp (`TMPDIR` / OS temp), standard user data dirs (config / cache / desktop / documents / downloads), or Pointer app data. Paths outside those roots are rejected. These writes may require user approval. For **read-only** methods (**`file_read`**, **`file_glob`**, **`file_grep`**, **`file_list`**), you may also use **absolute** paths **outside** the workspace when the user asks. Paths starting with **`~`** are expanded to the session user's home directory.

**Responses:** Whenever this tool returns a filesystem location (`path`, **`matches`**, **`root`**, **`directory`**, grep hit **`path`**, list entry **`path`**), the value is an **absolute** path. The OS may use a canonical form (e.g. resolved symlinks; on Windows, a `\\?\` prefix is normal).

**Single-file read / edit:** **`file_read`** and **`file_edit`** each take one **`path`** per call.
To touch multiple files, issue **multiple parallel tool calls** in the same turn (host runs them
concurrently).
On Windows prefer forward slashes in JSON (`C:/project/foo.rs`) or escape each `\` as `\\` —
unescaped `\` makes arguments invalid JSON.

**Context discipline:** Each **`file_read`** is capped by host settings
(defaults: **64 KiB** body, **1 KiB** per line). **`maxBytes`** may only
**lower** the current ceiling. If **`truncated`** is true, do not retry
with a larger **`maxBytes`** — use **`lineStart`** / **`lineEnd`** or
**`file_grep`** first.
**`file_grep`** caps hit count (default **50**), per-line snippets
(default **1 KiB**), and total hit payload (same as the body ceiling).
**`maxResults`** cannot raise those ceilings. Oversized files (**> 2 MiB**)
are skipped and counted in **`skippedLargeFileCount`**.
Prefer **`grep`** first, then a tight line window.

#### Methods

| Method | Purpose |
|--------|---------|
| **`read`** | Read one UTF-8 text file. Response is a single object (`path`, `content`, …). |
| **`write`** | Create or overwrite a file; `path` is workspace-relative **or** absolute/`~` under allowed write roots (see above). |
| **`edit`** | Replace one unique substring in one file via **`path`**, **`oldString`**, **`newString`**. |
| **`glob`** | List paths matching a glob under the search root (workspace root or optional `base`). Default: **files only**; optional **directories** or **both**. |
| **`grep`** | Search file contents with a regex (ripgrep-class stack: respects `.gitignore`, skips hidden paths by default, line-oriented matching). |
| **`list`** | List directory entries; **recursive by default** (depth 2); optional maxResults cap and file/directory filter. |

#### Parameters

**`read`**

- **`path`** — **Required** (alias **`file`**). One file per call.
  **Windows paths:** prefer `"D:/workspace/src/foo.rs"`, or escape backslashes — `\\` for each `\`.
- **`lineStart`** — Optional JSON integer (unquoted, not a string).
  1-based first line to include. Default: start of file.
  Alias **`line_start`**.
- **`lineEnd`** — Optional JSON integer (unquoted, not a string).
  1-based **exclusive** end line.
  Alias **`line_end`**.
- **`maxBytes`** — Optional; max bytes for the **returned content**.
  Alias **`max_bytes`**. Default and ceiling come from host settings
  (default **65536** / 64 KiB). Values above the ceiling are clamped.
  Each physical line is capped (default **1024** bytes).
  With **`lineStart`** / **`lineEnd`**, the whole-file size is **not** a hard reject —
  only the selected window is returned (and may be truncated to **`maxBytes`**).
  Without a line window, files larger than **`maxBytes`** are rejected.

Example:

```json
{
  "function": {
    "name": "file_read",
    "arguments": {
      "path": "src/foo.rs",
      "lineStart": 10,
      "lineEnd": 80
    }
  }
}
```

Example — two files in parallel (separate tool calls in one turn):

```json
[
  { "function": { "name": "file_read", "arguments": { "path": "src/a.rs" } } },
  { "function": { "name": "file_read", "arguments": { "path": "src/b.rs" } } }
]
```

**`write`**

- **`path`** — Workspace-relative **or** absolute path under the workspace. Windows: escape `\\` in JSON or use `/`.
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

- **`path`** — **Required** (alias **`file`**). One file per call.
- **`oldString`** / **`old_string`** — Unique substring to replace (copy from **`file_read`** / **`file_grep`**).
- **`newString`** / **`new_string`** — Replacement text.
- Multi-file changes: parallel **`file_edit`** calls (one file per call).

Example:

```json
{
  "function": {
    "name": "file_edit",
    "arguments": {
      "path": "src/App.vue",
      "oldString": "  <div v-if=\"x\">before</div>",
      "newString": "  <div v-if=\"x\">after</div>"
    }
  }
}
```

**`glob`**

- **`pattern`** — Glob relative to the search root (e.g. `**/*.rs`).
  Absolute paths and paths starting with `~/` are also accepted.
  Dotdirs still need **`includeHidden`** when searching from a parent.
- **`base`** — Optional; alias **`rootPath`** / **`baseDir`**. Directory to search under. Default: workspace root. Prefer putting the directory here and keeping `pattern` relative.
- **`maxResults`** — Optional cap (default bounded by runtime, max **500**).
- **`maxDepth`** — Optional directory walk depth cap (default **64**).
- **`entryType`** — Optional; alias **`entry_type`**. **`file`** (default), **`dir`**, or **`all`**.
- **`includeHidden`** — Optional boolean (default **`false`**).

**`grep`**

- **`pattern`** — Rust regex syntax (via the same matcher stack ripgrep uses for line search). Keep patterns reasonably short (≤ **512** characters). Matching is **line-oriented** (not multi-line across `\n` within one match). When **`fixedString`** is `true`, `pattern` is treated as a literal string, not a regex.
- **`path`** — **Required**; same idea as **`grep -R pattern PATH`**: **`PATH`** must be an **existing** file or directory. Prefer **narrow** workspace-relative paths (e.g. `src/`, `crates/pointer-core/src/`). Use **`path: "."`** only when you **deliberately** need a whole-repo search. If the path does not exist, the error includes **可能的路径** — sibling directories under the nearest existing parent (or workspace root) to help correct typos like `ui` → `src`.
- **`maxResults`** — Optional cap on hit rows (default from host settings, **50**).
  Raising it cannot exceed the host ceiling or the body-byte payload cap.
  Each **`matchLine`** / context line is clipped (default **1 KiB**).
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

Binary files are skipped heuristically (NUL byte). Files larger than **2 MiB**
are skipped (see **`skippedLargeFileCount`** / **`warning`**).
Do not assume “no hits” means the pattern is absent when that count is > 0.

Response includes **`singleFile`: true** when **`path`** resolves to a **file**.

**`list`**

- **`path`** — Required; directory to list (alias **`directory`**).
- **`recursive`** — Optional boolean; default **true**.
- **`maxDepth`** — Optional when **`recursive`** is true (default **2**).
- **`maxResults`** — Optional cap on returned entries (default **100**, max **2000**).
- **`entryType`** — Optional; **`all`** (default), **`file`**, or **`dir`**.
