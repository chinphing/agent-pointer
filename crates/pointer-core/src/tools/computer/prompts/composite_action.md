### composite_action

Use for **one-call combos** that achieve the goal in a single tool call: click+type, or move+scroll. Prefer this over calling **mouse** then **hotkey** or **mouse** multiple times when one composite call is enough.

**Call priority:** Prefer the fewest tool calls. Use **composite_action** first for “click and type” or “scroll at a specific element”; then hotkey or modified_click; then mouse. Use **wait** when a delay is needed.

**Reminder:** Click and type can be done **in one call** with composite_action (overlay-index: `type_text_at_index`; coordinate: `type_text_at`; or **`type_text_at_focused`** when the input already has focus). Do not call `mouse:click_index` then type separately — use one composite_action call.

**Overlay selection:** When labels are ambiguous, prefer the number drawn **inside** the target element’s box.

**Overlay-index methods:**
- **`type_text_at_index`** (`index`, `goal`, `text`, optional `clear_first`, optional `auto_enter`) — Clicks the element at index to focus, then types. Use when the target has an overlay number; do not call `mouse:click_index` first. If the target is already focused, or already focused with text visibly selected, do not call this method — use `type_text_at_focused` instead. `clear_first`: if true, selects all then types (replacing existing content). `auto_enter`: if true, presses Enter after typing.
- **`scroll_at_index`** (`index`, `goal`, `lines`) — Moves to the element at index and scrolls there. Choose `lines` the same way as **mouse:scroll_at_current**; the runtime does not rescale it.

**Mandatory reminder after `scroll_at_index`:** Same as `mouse:scroll_at_current` — judge movement from the next screenshot, not from tool text.

**Coordinate methods:**
- **`type_text_at`** (`goal`, `x`, `y`, `text`, optional `clear_first`, optional `auto_enter`) — Clicks at coordinates then types. Use when the target has no overlay number; **x, y** use the **same numeric system** as the current screen inject (see **mouse** coordinate rules). If the target is already focused, or already focused with text visibly selected, do not call this method — use `type_text_at_focused` instead. `clear_first`: if true, selects all then types (replacing existing content). `auto_enter`: if true, presses Enter after typing.

**Other (focused field, no overlay pick or typed point):**
- **`type_text_at_focused`** (`goal`, `text`, optional `clear_first`, optional `auto_enter`) — Types into the **currently focused** input field (no click). Use when the input already has focus (e.g. cursor is in the field, or you just focused it). `clear_first`: if true, selects all then types (replacing existing content). `auto_enter`: if true, presses Enter after typing.
- If the input already has focus and the existing text is visibly selected, type directly without `clear_first`; the new text should replace the current selection.

Parameter constraints:
- **`goal`** is required. Lead with the visible outcome this step is for, not "click..." alone. Name the target only to disambiguate. See **Communication** → **Action policy**.
- **`action`** is required. See **Communication** → **Action description in tool_args**.
- `auto_enter` defaults to **false**. Set it to **true** only when Enter is intended as the next action. On **Windows PowerShell / terminal** input, set `auto_enter=true` so the command executes immediately after typing (the tool blocks until after submit before returning — do not duplicate that delay yourself). If the next screenshot still shows no new output and no new prompt line, **do not press Enter again** — Enter was already sent; use **`wait:wait`** if more time is needed.
- For **`scroll_at_index`**, prefer an anchor **inside** the scrollable region (e.g. a list item); avoid large headings outside the viewport.
