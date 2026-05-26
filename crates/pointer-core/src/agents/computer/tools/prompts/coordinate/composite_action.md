### composite_action

Use for **one-call combos**: click+type, or focus-then-type.

Use coordinate methods as the default route in this guide. Use **`x`/`y`** from **`Overlay reference bboxes`** lookup.

**Call priority:** Use **composite_action** when this turn must **type literal `text` into a field**. For click-only steps, use **`mouse:click_at`**.

**Coordinate methods:**
- **`composite_action:type_text_at`** (`goal`, `action`, `x`, `y`, `text`, optional `clear_first`, optional `auto_enter`) — Clicks at **(x,y)** then types. If the field already has focus, use **`type_text_at_focused`**.
- **`composite_action:type_text_at_focused`** (`goal`, `action`, `text`, optional `clear_first`, optional `auto_enter`)

**Scroll:** **`mouse:hover_at`** or **`mouse:click_at`** in the scroll region, then **`mouse:scroll_at_current`**.

Parameter constraints:
- **`goal`**, **`action`**, **`text`** required where applicable.
- `auto_enter` defaults to **false**. On **Windows PowerShell / terminal**, set `auto_enter=true` when Enter should run the command.

**Optional `wait` in `tool_args`:** **~2–4** s after type-and-submit; **~1–2** s for focus+type.
