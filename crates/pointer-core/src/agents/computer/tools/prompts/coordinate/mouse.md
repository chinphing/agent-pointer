### mouse

Use for a single mouse action: click, double-click, right-click, hover, drag, scroll at the current cursor, or a small offset move.

**This session uses coordinate methods only.** Pick **reference index R** on the overlay image, then derive integer **`(x,y)`** from **`Overlay reference bboxes`** in **`[CUR_SCREEN]`**. **Forbidden:** `index`, `indices`, `from_index`, `to_index`, and all **`*_index`** methods in `tool_args`.

**Call priority:** Prefer **composite_action**, **hotkey**, or **modified_click** when one call achieves the same goal with fewer steps — unless this turn is **mouse** click-only.

**Coordinate methods** (require `goal`, `action`, `x`, `y` unless noted): **`mouse:click_at`**, **`mouse:double_click_at`**, **`mouse:right_click_at`**, **`mouse:hover_at`**.

**Drag:**
- **`mouse:drag_from_to_at`** — `goal`, `action`, **`x1`**, **`y1`**, **`x2`**, **`y2`** from bbox lookup.

Optional **`human_like`** (bool) on drag.

**Current cursor:** **`mouse:click_current`**, **`mouse:double_click_current`**, **`mouse:right_click_current`** — only **`goal`**, **`action`**.

**Other:** **`mouse:scroll_at_current`** (`goal`, `action`, `lines`).

**Offset move:** **`mouse:move_offset`** (`goal`, `action`, `dx`, `dy`).

Parameter constraints:
- **`goal`** and **`action`** are required.
- **`x`**, **`y`** must match **Location** line **3** from **`Overlay reference bboxes`** this turn.

**Optional `wait` in `tool_args`:** **~1–2** s for simple clicks; **~3–5** s for dialogs.

**Scroll workflow:** **`mouse:hover_at`** or **`mouse:click_at`** in the scroll region, then **`mouse:scroll_at_current`** if needed.

#### JSON example — `mouse:click_at`

```json
{
  "thoughts": "… Location line 3 therefore (x,y) ≈ (520, 880). …",
  "headline": "Click control",
  "tool_name": "mouse:click_at",
  "tool_args": {
    "goal": "Activate the highlighted button",
    "action": "click the blue primary button labeled Save in the dialog footer",
    "x": 520,
    "y": 880,
    "wait": "2"
  }
}
```
