### input_index / input_at / input_focused

Flat input tools — each tool name is the complete operation. No `method` parameter.

Every call requires **`goal`** (outcome) and **`action`** (human-readable description for UI).
Optional: `clear_first`, `auto_enter`, `wait` (1–5 s).

**One-call click + type (default for unfocused fields)**

**`input_index`** and **`input_at`** run in **one** tool call:

1. Click the target to focus
2. Optionally select-all when **`clear_first=true`**
3. Type **`text`**
4. Optionally press Enter when **`auto_enter=true`**

**Forbidden:** **`mouse_click_*`** this turn to focus, then **`input_*`** next
turn to type the same field. Put **`text`**, **`clear_first`**, and
**`auto_enter`** on the **same** **`input_index`** / **`input_at`** call.

**Index-based** (overlay digit — clicks bbox **center**):
- **`input_index`** — Click index, then type. Requires **`index`**, **`text`**.

**Coordinate-based** (session 0–1000):
- **`input_at`** — Click at **(x,y)**, then type. Requires **`x`**, **`y`**, **`text`**.

**Focused field:**
- **`input_focused`** — Type into focused input (no click). Requires **`text`** only.

**Tool choice (prefer fewer steps):**
- Need to type/replace in a field **this turn** and it is **not** focused
  → **`input_index`** or **`input_at`** with **`text`** — **not**
  **`mouse_click_*`** alone.
- Field already focused (cursor visible, text selected, placeholder highlighted)
  → **`input_focused`** with **`text`** only — **never** **`input_at`** /
  **`input_index`** (those click at coordinates or bbox center first and can
  miss the caret).
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
