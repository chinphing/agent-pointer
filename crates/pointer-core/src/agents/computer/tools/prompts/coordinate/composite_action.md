### composite_action

Use for **one-call combos**: click+type, or focus-then-type.

Use coordinate methods as the default route in this guide. Use **`x`/`y`** from **`Overlay reference bboxes`** lookup.

**Call priority:** Prefer the fewest tool calls. Use **composite_action** first
when one call can finish the step (for example click+type or focus+type), then
**hotkey** / **modified_click**, then **mouse**. Use **wait** only when delay is needed.

**Coordinate methods:**
- **`composite_action:type_text_at`** (`goal`, `action`, `x`, `y`, `text`, optional `clear_first`, optional `auto_enter`) — Clicks at **(x,y)** then types. If the field already has focus, use **`type_text_at_focused`**.
- **`composite_action:type_text_at_focused`** (`goal`, `action`, `text`, optional `clear_first`, optional `auto_enter`)

**Browser URL workflow:** Prefer `hotkey` to focus the address bar first
(`cmd/ctrl+l` or `alt+d`), then call **`composite_action:type_text_at_focused`**.
Avoid click-based typing for URL entry when shortcut focus is available.

**Scroll:** **`mouse:hover_at`** or **`mouse:click_at`** in the scroll region, then **`mouse:scroll_at_current`**.

Parameter constraints:
- **`goal`**, **`action`**, **`text`** required where applicable.
- `clear_first` defaults to **false**. Set `clear_first=true` only when existing
  field content must be removed before typing (for example replace a preset
  value or overwrite prior input). Keep **false** when appending is intended.
- `auto_enter` defaults to **false**. On **Windows PowerShell / terminal**, set `auto_enter=true` when Enter should run the command.

**Optional `wait` in `tool_args`:** **~2–4** s after type-and-submit; **~1–2** s for focus+type.
