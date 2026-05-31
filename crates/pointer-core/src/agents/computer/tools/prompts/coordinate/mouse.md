### mouse

Use for a single mouse action: click, double-click, right-click, hover, drag, scroll at the current cursor, or a small offset move.

Use coordinate methods as the default route in this guide. Pick **reference index R** on the overlay image, then derive integer **`(x,y)`** from **`Overlay reference bboxes`** in **`[CUR_SCREEN]`**.

**Call priority:** Prefer **composite_action**, **hotkey**, or **modified_click** when one call achieves the same goal with fewer steps — unless this turn is **mouse** click-only.

**Coordinate methods** (require `goal`, `action`, `x`, `y` unless noted): **`click_at`**, **`double_click_at`**, **`right_click_at`**, **`hover_at`**.

**Drag:**
- **`drag_from_to_at`** — `goal`, `action`, **`x1`**, **`y1`**, **`x2`**, **`y2`** from bbox lookup.

Optional **`human_like`** (bool) on drag.

**Current cursor:** **`click_current`**, **`double_click_current`**, **`right_click_current`** — only **`goal`**, **`action`**.

**Other:** **`scroll_at_current`** (`goal`, `action`, `lines`).

**Offset move:** **`move_offset`** (`goal`, `action`, `dx`, `dy`).

Parameter constraints:
- **`goal`** and **`action`** are required.
- **`x`**, **`y`** must match **Location** line **3** from **`Overlay reference bboxes`** this turn.

**Optional `wait` in `tool_args`:** **~1–2 s** for simple clicks; **~3–5 s**
for dialogs, downloads, uploads, and navigation-triggered reloads.
If progress is visible but completion is not yet verifiable, keep Verify in a
non-terminal state (for example `action_result=pending`), then verify again after
waiting (for example in download or transfer history UI).

**Scroll workflow:** use **`hover_at`** or **`click_at`** in the scroll region, then **`scroll_at_current`** if needed.

