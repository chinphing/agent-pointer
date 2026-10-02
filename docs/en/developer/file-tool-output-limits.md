# Tool content limits (file read / search / terminal)

English | [简体中文](../../zh-CN/developer/file-tool-output-limits.md)

Defaults (apart from the file-read line count and the glob / list entry counts, these can be changed under **Settings → System settings** → content limits / terminal timeout):

| Item | Default | Settings field | Allowed range |
|----|------|----------|----------|
| File-read body | 64 KiB | `fileReadMaxBytes` | 4 KiB – 1 MiB |
| File-read line count | default 500, ceiling 2000 | (implementation constant, not a setting) | 1 – 2000 |
| Single line | 1 KiB | `fileLineMaxBytes` | 256 B – 16 KiB |
| Search hits | 50 | `fileGrepMaxResults` | 1 – 200 |
| glob hits | default 100, ceiling 500 | (implementation constant) | 1 – 500 |
| Directory listing | default 100, ceiling 2000 | (implementation constant) | 1 – 2000 |
| Terminal stdout/stderr (one stream each) | 16 KiB | `terminalOutputMaxBytes` | 4 KiB – 256 KiB |
| Terminal idle timeout (the default when `timeoutMs` is not passed) | 30 seconds | `terminalTimeoutSeconds` | 1 – 86400 seconds |
| Terminal wall-clock ceiling | 24 hours | `terminalMaxWallHours` | 1 – 10000 hours |

An agent cannot raise `maxBytes` / `limit` / `maxOutputBytes` / `maxWallMs` to break through the **current** ceiling.
Tool arguments can only lower it. The idle timeout is the exception: `timeoutMs` may exceed the settings default, with a hard ceiling of 86400 seconds.

Implementation: `crates/pointer-core/src/tools/file/{mod,read,grep,glob,list}.rs`,
`crates/pointer-core/src/tools/terminal.rs`;
settings live in `UserSettings`, shared by desktop and web.

## Behaviour

- **`file_read`**: every call has a line window. When not passed, **`offset` defaults to 1** and **`limit` defaults to 500**; the `limit` ceiling is **2000** (tool arguments cannot raise it further).
  Only that window is returned, and the body is still truncated by **`maxBytes`**. It stops once the window is read and no longer scans the whole file.
  The only public names are **`offset` / `limit`** (`offset` is the 1-based start line, `limit` is the maximum number of lines returned, a JSON **integer**).
  The schema has `additionalProperties: false`, and at runtime `lineStart` / `lineEnd` / `startLine` / `endLine` are no longer accepted either.
- **`file_grep`**: the tool argument for the hit count is **`limit`** (defaults to the same value as the **`fileGrepMaxResults`** setting, **50**; ceiling **200**, which tool arguments cannot raise).
  The single-line snippet and the total hit payload (the same ceiling as the body) are both truncated.
- **`file_glob`**: the tool argument for the hit count is **`limit`** (default **100**, ceiling **500**).
- **`file_list`**: the tool argument for the number of entries returned is **`limit`** (default **100**, ceiling **2000**).
- Files **> 2 MiB** are still skipped (implementation constant, not counted as a setting) and counted in `skippedLargeFileCount`.
- **`terminal`**: stdout / stderr are **each** truncated to the ceiling; over the limit the **tail is kept and the head dropped**, with the prefix
  `...[output truncated]`. The tool result returned to the model is truncated this way.
  The frontend live buffer keeps the tail at the same ceiling, with the same prefix. Once the command ends, whether this buffer survives along with the arguments and result follows the body rules — see
  [`../ui/tool-payload-memory.md`](../../zh-CN/ui/tool-payload-memory.md).
  Idle timeout: when `timeoutMs` is not passed the settings default is used; a tool argument may specify 1s–86400s, not capped by the settings default.
  The wall-clock ceiling is controlled by settings; the tool's `maxWallMs` can only lower it.

## Observability

Truncation and skipped large files log **info / warn**, and responses carry `truncated` / `warning`
(for the terminal, `stdoutTruncated` / `stderrTruncated`).
