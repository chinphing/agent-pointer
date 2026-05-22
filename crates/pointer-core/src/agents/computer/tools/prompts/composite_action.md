### composite_action

Use for **one-call combos**: click+type, or focus-then-type. Prefer this over **mouse** then **hotkey** when one composite call is enough.

**Call priority:** Use **composite_action** when **`Next:`** **`this turn:`** requires **typing literal `text` into a field**. For **click / press / toggle / icon-only** steps, use **`mouse:click_at`** per **`Tool route:`** — not **`type_text_at_*`**.

**`*_index` methods are disabled this session.** Use coordinate methods only; **`x`/`y`** from **`Overlay reference bboxes`** lookup in **Location** line **3** (see **Coordinate source** in communication rules).

**Coordinate methods:**
- **`composite_action:type_text_at`** (`goal`, `action`, `x`, `y`, `text`, optional `clear_first`, optional `auto_enter`) — Clicks at **Location** **(x,y)** (must trace to **`Overlay reference bboxes row R`**) then types. **`Pointer:`** must judge click aim on **`[Zoom pointer before action]`** (same as **`click_at`**), not **`n/a`**. If the field already has focus (cursor visible), use **`type_text_at_focused`** instead. `clear_first`: select all then type. `auto_enter`: press Enter after typing.

**Focused field (no new aim):**
- **`composite_action:type_text_at_focused`** (`goal`, `action`, `text`, optional `clear_first`, optional `auto_enter`) — Types into the **currently focused** input (no click). Use when focus is already in the field.

**Scroll:** There is no index-based scroll method. Use **Location** → **`mouse:hover_at`** or **`mouse:click_at`** inside the scroll region, then **`mouse:scroll_at_current`**.

Parameter constraints:
- **`goal`**, **`action`**, **`text`** required for **`type_text_at_*`** as applicable. **`goal`** = outcome; **`action`** = visible target (match **`Tool route:`** line **2** wording).
- `auto_enter` defaults to **false**. On **Windows PowerShell / terminal**, set `auto_enter=true` when Enter should run the command.

**Optional `wait` in `tool_args`:** After successful calls (1–5 s; distinct from standalone **`wait`** tool). Heuristic: type-and-submit **~2–4** s; simple focus+type **~1–2** s.
