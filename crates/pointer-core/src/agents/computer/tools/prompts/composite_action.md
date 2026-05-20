### composite_action

Use for **one-call combos**: click+type, or focus-then-type. Prefer this over **mouse** then **hotkey** when one composite call is enough.

**Call priority:** Use **composite_action** when **`Next:`** **`this turn:`** requires **typing literal `text` into a field**. For **click / press / toggle / icon-only** steps, use **`mouse:click_at`** per **Communication §6** — not **`type_text_at_*`**.

**`*_index` methods are disabled this session.** Use coordinate methods only (see **Communication § Tool geometry**).

**Coordinate methods:**
- **`composite_action:type_text_at`** (`goal`, `action`, `x`, `y`, `text`, optional `clear_first`, optional `auto_enter`) — Clicks at **Location** **(x,y)** then types. If the field already has focus (cursor visible), use **`type_text_at_focused`** instead. `clear_first`: select all then type. `auto_enter`: press Enter after typing.

**Focused field (no new aim):**
- **`composite_action:type_text_at_focused`** (`goal`, `action`, `text`, optional `clear_first`, optional `auto_enter`) — Types into the **currently focused** input (no click). Use when focus is already in the field.

**Scroll:** There is no index-based scroll method. Use **Location** → **`mouse:hover_at`** or **`mouse:click_at`** inside the scroll region, then **`mouse:scroll_at_current`**.

Parameter constraints:
- **`goal`**, **`action`**, **`text`** required for **`type_text_at_*`** as applicable. See **Communication** → **Action policy** / **Action description in tool_args**.
- `auto_enter` defaults to **false**. On **Windows PowerShell / terminal**, set `auto_enter=true` when Enter should run the command.

**Optional `wait` in `tool_args`:** See **Communication** → **Post-action `wait` in `tool_args`**. Heuristic: type-and-submit **~2–4** s; simple focus+type **~1–2** s.
