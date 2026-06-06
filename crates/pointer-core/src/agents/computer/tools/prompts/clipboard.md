### clipboard_read / clipboard_write

Flat clipboard tools — each tool name is the complete operation. No `method` parameter.

Read or set the **system clipboard** as **plain text**. Does **not** paste into a field by itself.

- **`clipboard_read`** — Requires **`goal`** only. Returns current clipboard text in the tool reply (very long content may be truncated). **Mandatory after any copy** — UI copy button, context-menu Copy, or **`hotkey`** Copy (`command,c` / `ctrl,c`). Screenshots cannot show clipboard contents; **do not guess** and **do not copy again** until you have read once.

- **`clipboard_write`** — Requires **`goal`** and **`text`**. Puts `text` on the clipboard. To insert into the focused field, follow with **`hotkey`** paste or **`input_focused`** as appropriate.

**Copy → read → use (mandatory chain)**

1. Copy once (`hotkey` Copy or click Copy / 复制).
2. **`clipboard_read`** on the **next** turn — confirm non-empty text matches intent.
3. Only then paste (`command,v`) or **`clipboard_write`** elsewhere.

**Forbidden:** repeating Copy or re-clicking Copy before **`clipboard_read`**. If read is empty or wrong, fix selection/focus first — do not spam Copy.

**Call priority:** Prefer **input_focused** / **hotkey** for normal typing when you already know the string. Use **`clipboard_write`** when the clipboard must be an intermediate.

**Note:** Binary or rich clipboard formats are not exposed — text only. On some **Linux** sessions (e.g. **Wayland** without a running clipboard portal), reads/writes may fail; the tool returns an error message instead of guessing.

**`action` field:** Required when your desktop instructions ask for **`action`** on tool calls; describe the visible target or intent in words, **not** overlay indices.
