### clipboard

Read or set the **system clipboard** as **plain text**. Does **not** paste into a field by itself.

**Methods:**

- **`read`** — `goal` only. Returns current clipboard text in the tool reply (very long content may be truncated). Use after **Copy** or when you must **ground** clipboard state (do not guess from screenshots).

- **`write`** — `goal` and **`text`**. Puts `text` on the clipboard. To insert into the focused field, follow with **hotkey** paste or **composite_action** as appropriate.

**Call priority:** Prefer **composite_action** / **hotkey** for normal typing when you already know the string. Use **`write`** when the clipboard must be an intermediate. Use **`read`** after copy-like actions when the UI gives **no** reliable visible confirmation.

**Note:** Binary or rich clipboard formats are not exposed — text only. On some **Linux** sessions (e.g. **Wayland** without a running clipboard portal), reads/writes may fail; the tool returns an error message instead of guessing.

**`action` field:** Required when your desktop instructions ask for **`action`** on tool calls; describe the visible target or intent in words, **not** overlay indices.
