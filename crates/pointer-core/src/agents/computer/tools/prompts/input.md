### input_index / input_at / input_focused

Flat input tools — each tool name is the complete operation. No `method` parameter.

Every call requires **`goal`** (outcome) and **`action`** (human-readable description for UI).
Optional: `clear_first`, `auto_enter`, `wait` (1–5 s).

**Index-based** (overlay digit — clicks bbox **center**):
- **`input_index`** — Type text at index. Requires **`index`**, **`text`**.

**Coordinate-based** (session 0–1000):
- **`input_at`** — Type text at coordinates. Requires **`x`**, **`y`**, **`text`**.

**Focused field:**
- **`input_focused`** — Type into focused input. Requires **`text`** only.

**Tool choice (prefer fewer steps):**
- Field already focused (cursor visible, text selected) → **`input_focused`**
  — not **`input_at`** / **`input_index`** (those click bbox center first).
- Shortcut can focus the field (e.g. address bar) → **`hotkey`** then
  **`input_focused`** on the next turn when focus is confirmed.

**Optional args:**

- **`clear_first`** (default **`false`**): set **`true`** only when replacing
  all existing field content. If text is already selected in a focused field,
  type without **`clear_first`** — selection is replaced by typing.
- **`auto_enter`** (default **`false`**): set **`true`** when Enter should
  submit or confirm (search, send, dialog OK, form submit).
  On terminal / PowerShell input, set **`true`** so the command runs after
  typing. If the tool used **`auto_enter=true`**, do **not** press Enter
  again — use **`wait`** when the UI still looks unchanged.
- **`wait`**: optional delay (1–5 s) after typing completes. Omit when the
  screen already shows the expected result.
  Heuristic:
  - **~1–2 s** — local field feedback (autocomplete, inline validation,
    dropdown opening, debounced search while typing).
  - **~2–3 s** — after **`auto_enter`** for light submit (send message,
    confirm dialog, simple search, terminal command with quick output).
  - **~3–5 s** — submit that triggers navigation, app launch, page reload,
    heavy paste/import, or slow shell jobs before the next verify.
  If the UI is still unsettled after **5 s**, use the standalone **`wait`**
  tool on the next turn instead of stacking long **`wait`** on every input call.
