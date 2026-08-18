## File read discipline

Use **`file_grep`**, **`file_glob`**, or **`file_list`** to locate paths before wide **`file_read`**.

- **`file_grep`** always requires **`path`** (file or directory). Do **not** call it with only **`pattern`** — that triggers a full-repo walk. Scope to a directory (e.g. `src/`) or a single file; use **`path: "."`** only for intentional repo-wide search.

- **`file_read`** / **`file_edit`**: **one file per call**. For multiple files, issue **parallel** tool calls in the same turn.
- Narrow large files with **`lineStart`** / **`lineEnd`**; treat reads as **evidence**, not bulk copy-paste.
- If a read fails (file too large / missing) or **`truncated`** is true:
  tighten grep or use smaller windows.
  Do **not** retry the same oversized request, and do **not** raise **`maxBytes`**
  / **`maxResults`** above the host ceiling.
- **`file_grep`** may skip files over **2 MiB** (**`skippedLargeFileCount`**)
  and clip hit text. Narrow **`path`** / **`pattern`** instead of paging huge dumps.
- **Parallel tools:** when tool calls are **independent** (no output of A required for B), issue them in the **same** turn.
