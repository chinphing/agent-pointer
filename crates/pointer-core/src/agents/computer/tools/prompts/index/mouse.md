### mouse

Use for a single mouse action: click, double-click, right-click, hover, drag, scroll at the current cursor, or a small offset move.

Use overlay indices as the default route in this guide. Target the numbered region on **`[Annotated after action]`**.

**Call priority:** Prefer **composite_action**, **hotkey**, or **modified_click** when one call achieves the same goal with fewer steps.

**Index methods** (require `goal`, `action`, `index`):
- **`click_index`**, **`double_click_index`**, **`right_click_index`**, **`hover_index`**
- **`drag_from_to_index`** — `from_index`, `to_index` on the current annotated frame

Use the chosen overlay region center directly. Paste **`- R: (left, top, right, bottom)`** verbatim when needed (**MA-3 FOUND**). **Verify:** UI outcome vs **goal** — pointer on target alone is **not** **pass** for click/copy goals.

When the control is **inside** a large bbox, prefer a tighter dedicated index. When it sits **outside** the bbox border, switch to a tighter index or use coordinate tools instead of index-offset math.

Optional **`human_like`** (bool) on index clicks and drags.

**Current cursor (no index move):** **`click_current`**, **`double_click_current`**, **`right_click_current`** — only **`goal`**, **`action`**. Use when the cursor is already on the target.

**Other:** **`scroll_at_current`** (`goal`, `action`, `lines`) — cursor must already be inside the scrollable area.

**Offset move:** **`move_offset`** (`goal`, `action`, `dx`, `dy`) — nudge from current position.

Parameter constraints:
- **`goal`** and **`action`** are required.
- **`index`** must match a visible overlay digit on the current annotated screenshot.

**Optional `wait` in `tool_args`:** After successful calls (1–5 s). Heuristic:
**~1–2 s** for simple clicks; **~3–5 s** for dialogs, downloads, uploads, and
navigation-triggered reloads.
If progress is visible but completion is not yet verifiable, keep Verify in a
non-terminal state (for example `action_result=pending`), then verify again after
waiting (for example in download or transfer history UI).

**Reposition:** **MA-3 NOT FOUND** → **`hover_index`** on **R** center — no click/type that turn.

**Scroll workflow:** **`hover_index`** or **`click_index`** on the scroll region, then **`scroll_at_current`** if needed.
