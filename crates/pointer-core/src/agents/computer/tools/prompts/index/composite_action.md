### composite_action

Use for **one-call combos**: click+type, or focus-then-type.

Use overlay indices as the default route in this guide.

**Call priority:** Prefer the fewest tool calls. Use **composite_action** first
when one call can finish the step (for example click+type, focus+type, or
`scroll_at_index`), then **hotkey** / **modified_click**, then **mouse**.
Use **wait** only when delay is needed.

**Index methods:**
- **`type_text_at_index`** (`goal`, `action`, `index`, `text`, optional `clear_first`, optional `auto_enter`, optional `anchor`, `dx`, `dy`) — Clicks the indexed region then types. If the field already has focus, use **`type_text_at_focused`** instead.
- **`scroll_at_index`** (`goal`, `action`, `index`, `lines`) — Move to the indexed scroll region, then scroll.

**Focused field (no new click):**
- **`type_text_at_focused`** (`goal`, `action`, `text`, optional `clear_first`, optional `auto_enter`) — Types into the focused input.

**Browser URL workflow:** Prefer `hotkey` to focus the address bar first
(`cmd/ctrl+l` or `alt+d`), then call **`type_text_at_focused`**.
Avoid click-based typing for URL entry when shortcut focus is available.

Parameter constraints:
- **`goal`**, **`action`**, **`text`** required where applicable.
- `clear_first` defaults to **false**. Set `clear_first=true` only when existing
  field content must be removed before typing (for example replace a preset
  value or overwrite prior input). Keep **false** when appending is intended.
- `auto_enter` defaults to **false**. On **Windows PowerShell / terminal**, set `auto_enter=true` when Enter should run the command.

**Optional `wait` in `tool_args`:** **~2–4 s** after type-and-submit;
**~1–2 s** for focus+type.
Use the upper range when submit starts export/upload/download or page reload.
If the page is still transitioning after this settle wait, use standalone
`wait` and verify completion from history/result surfaces before concluding.
