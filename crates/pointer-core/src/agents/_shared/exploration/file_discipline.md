## File read discipline

Use **`file_grep`**, **`file_glob`**, or **`file_list`** to locate paths before wide **`file_read`**.

- Batch reads: use **`paths`** (array of objects with **`path`**, optional **`lineStart`** / **`lineEnd`**) when you have **two or more** files in one step.
- Narrow large files with line ranges; treat reads as **evidence**, not bulk copy-paste.
- If output is **`truncated`**, **`batchCapped`**, or **`error`**: split batches, tighten grep, or use smaller windows—do not repeat the same oversized batch.
- **Parallel tools:** when tool calls are **independent** (no output of A required for B), issue them in the **same** turn.
