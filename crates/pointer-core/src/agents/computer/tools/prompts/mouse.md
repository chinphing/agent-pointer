### mouse

Use for a single mouse action: click, double-click, right-click, hover, drag, scroll at the current cursor, or a small offset move.

**`*_index` methods are disabled this session.** Use **coordinate methods** at **`Location:`** line **3** **`(x,y)`** (see **Tool geometry** in communication rules). Overlay digits are reference anchors only — **never** **`index`** in **`tool_args`**.

**Call priority:** Prefer **composite_action**, **hotkey**, or **modified_click** when one call achieves the same goal with fewer steps — unless **`Next:`** / **`Tool route:`** fixes this turn as **mouse** click-only (icons/buttons).

**Coordinate methods** (require `goal`, `action`, `x`, `y` unless noted): **`mouse:click_at`**, **`mouse:double_click_at`**, **`mouse:right_click_at`**, **`mouse:hover_at`**.

**Drag (left button down → move → up):**
- **`mouse:drag_from_to_at`** — `goal`, `action`, **`x1`**, **`y1`** (press here), **`x2`**, **`y2`** (release here). Compute both points via **Location** / **Overlay reference bboxes** (two reference indices or corner+offset).

Optional **`human_like`** (bool) on drag — smoothed move to start and eased drag.

**Current cursor (no move):** **`mouse:click_current`**, **`mouse:double_click_current`**, **`mouse:right_click_current`** — only **`goal`**, **`action`**. Does **not** move the pointer; use when the cursor is **already** on the target (**POINTER** / prior step).

**Other:** **`mouse:scroll_at_current`** (`goal`, `action`, `lines`) — use when the mouse is **already** inside the scrollable area. **`lines`** is an approximate **line count** (positive = up, negative = down).

**Choosing `lines`:** Estimate visible rows and choose a signed line count with sensible overlap. Your value is used as given (not auto-rescaled).

**Mandatory reminder after any scroll tool call:** On the next turn, compare new `[CUR_SCREEN]` to prior frames. **No visible change** = scroll failed; change anchor or tactic.

**Offset move:** **`mouse:move_offset`** (`goal`, `action`, `dx`, `dy`) — nudge from **current** position (right/down positive).

Parameter constraints:
- **`goal`** and **`action`** are required for all methods. **`goal`** = outcome; **`action`** = visible target (match **`Tool route:`** line **2** wording).
- For coordinate methods: supply **`x`**, **`y`** per this session’s coordinate rules; do not reuse numbers from an older turn’s image.

**Optional `wait` in `tool_args`:** After successful calls (1–5 s; distinct from standalone **`wait`** tool). Heuristic: **~1–2** s for simple clicks/hovers; **~3–5** s for dialogs or heavy repaints.

**Scroll workflow:** When the cursor is not yet in the scroll region, run **Location** → **`mouse:hover_at`** or **`mouse:click_at`** at anchor **(x,y)** inside the scrollable area, then **`mouse:scroll_at_current`** on a follow-up turn if needed.

#### JSON example — `mouse:click_at`

```json
{
  "thoughts": "… Location line 3 therefore (x,y) ≈ (520, 880). Recheck coordinates: … proceed. Tool route line 2 mouse:click_at x:520 y:880 …",
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
