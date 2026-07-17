## File read discipline

Use **`file_grep`**, **`file_glob`**, or **`file_list`** to locate paths before wide **`file_read`**.

- **`file_grep`** always requires **`path`** (file or directory). Do **not** call it with only **`pattern`** — that triggers a full-repo walk. Scope to a directory (e.g. `src/`) or a single file; use **`path: "."`** only for intentional repo-wide search.

- **`file_read`** / **`file_edit`**: **one file per call**. For multiple files, issue **parallel** tool calls in the same turn.
- Narrow large files with **`lineStart`** / **`lineEnd`**; treat reads as **evidence**, not bulk copy-paste.
- If a read fails (file too large / missing): tighten grep or use smaller windows—do not retry the same oversized request.
- **Parallel tools:** when tool calls are **independent** (no output of A required for B), issue them in the **same** turn.
