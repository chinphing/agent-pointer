### composite_action

Use for **one-call combos** that achieve the goal in a single tool call: click+type, or move+scroll. Prefer this over calling **mouse** then **hotkey** or **mouse** multiple times when one composite call is enough.

**Call priority:** Prefer the fewest tool calls. Use **composite_action** when **`Next:`** **`this turn:`** requires **typing literal `text` into a field** or **scroll at an overlay index** — see **Communication** **§6 Tool route**. For **click / press / toggle / icon-only** steps (copy icon, submit button, …), use **`mouse`** **`click_*`** per the **mouse** tool prompt, not **`type_text_at_*`**.

**Reminder:** Click and type can be done **in one call** (overlay-index: **`composite_action:type_text_at_index`**; coordinate: **`composite_action:type_text_at`**; or **`composite_action:type_text_at_focused`** when the input already has focus). Do not call **`mouse:click_index`** then type separately — use one composite_action call **only when `text` is required this turn**.

**Overlay selection:** When labels are ambiguous, prefer the number drawn **inside** the target element’s box.

**Overlay-index methods:**
- **`composite_action:type_text_at_index`** (`index`, `goal`, `text`, optional `clear_first`, optional `auto_enter`) — Clicks the element at index to focus, then types. Use when the target has an overlay number; do not call **`mouse:click_index`** first. If the target is already focused, or already focused with text visibly selected, use **`composite_action:type_text_at_focused`** instead. `clear_first`: if true, selects all then types (replacing existing content). `auto_enter`: if true, presses Enter after typing.
- **`composite_action:scroll_at_index`** (`index`, `goal`, `lines`) — Moves to the element at index and scrolls there. Choose `lines` the same way as **`mouse:scroll_at_current`**; the runtime does not rescale it.

**Mandatory reminder after `composite_action:scroll_at_index`:** Same as **`mouse:scroll_at_current`** — judge movement from the next `[CUR_SCREEN]` images, not from tool text.

**Coordinate methods:**
- **`composite_action:type_text_at`** (`goal`, `x`, `y`, `text`, optional `clear_first`, optional `auto_enter`) — Clicks at coordinates then types. Use when the target has no overlay number; **`x`, `y`** follow the **mouse** tool’s coordinate rules for this turn. If the target is already focused, or already focused with text visibly selected, use **`composite_action:type_text_at_focused`** instead. `clear_first`: if true, selects all then types (replacing existing content). `auto_enter`: if true, presses Enter after typing.

**Other (focused field, no overlay pick or typed point):**
- **`composite_action:type_text_at_focused`** (`goal`, `text`, optional `clear_first`, optional `auto_enter`) — Types into the **currently focused** input field (no click). Use when the input already has focus (e.g. cursor is in the field, or you just focused it). `clear_first`: if true, selects all then types (replacing existing content). `auto_enter`: if true, presses Enter after typing.
- If the input already has focus and the existing text is visibly selected, type directly without `clear_first`; the new text should replace the current selection.

Parameter constraints:
- **`goal`** is required. Lead with the visible outcome this step is for, not "click..." alone. Name the target only to disambiguate. See **Communication** → **Action policy**.
- **`action`** is required. See **Communication** → **Action description in tool_args**.
- **`text`** is required for all **`type_text_at_*`** methods. Prefer a JSON **string** (e.g. `"13856729034"`). Whole numbers are also accepted and coerced to digits-only text.
- `auto_enter` defaults to **false**. Set it to **true** only when Enter is intended as the next action. On **Windows PowerShell / terminal** input, set `auto_enter=true` so the command executes immediately after typing (the tool blocks until after submit before returning — do not duplicate that delay yourself). If the next screenshot still shows no new output and no new prompt line, **do not press Enter again** — Enter was already sent; use **wait** if more time is needed.
- For **`composite_action:scroll_at_index`**, prefer an anchor **inside** the scrollable region (e.g. a list item); avoid large headings outside the viewport.

**Optional `wait` in `tool_args`:** See **Communication** (desktop agent) → **Post-action `wait` in `tool_args`**. Heuristic: type-and-submit flows often **~2–4** s before the canvas updates; simple focus+type often **~1–2** s.
