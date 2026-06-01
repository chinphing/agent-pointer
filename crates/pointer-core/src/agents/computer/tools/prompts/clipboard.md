### clipboard_read / clipboard_write

Flat clipboard tools — each tool name is the complete operation. No `method` parameter.

Read or set the **system clipboard** as **plain text**. Does **not** paste into a field by itself.

- **`clipboard_read`** — Requires **`goal`** only. Returns current clipboard text in the tool reply (very long content may be truncated). Use after **Copy** or when you must **ground** clipboard state (do not guess from screenshots).

- **`clipboard_write`** — Requires **`goal`** and **`text`**. Puts `text` on the clipboard. To insert into the focused field, follow with **`hotkey`** paste or **`input_focused`** as appropriate.

**Call priority:** Prefer **input_focused** / **hotkey** for normal typing when you already know the string. Use **`clipboard_write`** when the clipboard must be an intermediate. Use **`clipboard_read`** after copy-like actions when the UI gives **no** reliable visible confirmation.

**Note:** Binary or rich clipboard formats are not exposed — text only. On some **Linux** sessions (e.g. **Wayland** without a running clipboard portal), reads/writes may fail; the tool returns an error message instead of guessing.

**`action` field:** Required when your desktop instructions ask for **`action`** on tool calls; describe the visible target or intent in words, **not** overlay indices.
