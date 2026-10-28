### input_at / input_focused

Each tool name is the complete operation — no `method` parameter.
**`input_focused`** needs no coordinates — it types into whatever already has
focus.

**One-call click + type (default for unfocused fields)**

**`input_at`** runs in **one** tool call:

1. Click the target to focus
2. Optionally select-all when **`clear_first=true`**
3. Type **`text`**
4. Optionally press Enter when **`auto_enter=true`**

**Forbidden:** **`mouse_click_*`** this turn to focus, then **`input_*`** next
turn to type the same field. Put **`text`**, **`clear_first`**, and
**`auto_enter`** on the **same** **`input_at`** call.

**Grounded** (0–1000 normalized, origin at the screenshot's top-left):
- **`input_at`** — Ground **`target_desc`** inside **`bbox`**, click the confirmed
  centre, then type. Requires **`target_desc`**, **`bbox`**, **`text`**.
  Both parts are required: `bbox` narrows the search, `target_desc` says which
  control it is. A loose box is fine — grounding re-finds the field inside it.
  There is no raw-coordinate form.

**`target_desc`** follows **Writing `target_desc`** in your system prompt —
three parts, in one sentence. Several inputs can share a placeholder; the
screen position is what tells them apart.

**Write `target_desc` in the language of the UI.** Chinese labels get a Chinese
description — a translated element name does not match what the grounder reads
on screen.

Coordinates are **0–1000 normalized** — see **Locating targets**.

**Focused field:**
- **`input_focused`** — Type into focused input (no click). Requires **`text`** only.

**Tool choice (prefer fewer steps):**
- Need to type/replace in a field **this turn** and it is **not** focused
  → **`input_at`** with **`text`** — **not**
  **`mouse_click_*`** alone.
- Field already focused (cursor visible, text selected, placeholder highlighted)
  → **`input_focused`** with **`text`** only — **never** **`input_at`**
  (it clicks at coordinates first and can miss the caret).
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
  screen already shows the expected result. Rule of thumb: **1–2 s** for local
  field feedback, **2–3 s** after `auto_enter` on a light submit, **3–5 s** when
  the submit triggers navigation, app launch, or a slow shell job. If the UI is
  still unsettled after 5 s, use the standalone **`wait`** tool next turn
  instead of stacking long `wait` on every call.
