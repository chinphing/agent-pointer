### mouse

Use for a single mouse action: click, double-click, right-click, hover, drag, scroll at the current cursor, or a small offset move. Prefer **overlay-index methods** when the target has an overlay number; otherwise use **coordinate methods** from the current inject scale. When multiple labels are plausible, prefer the badge drawn **inside** the target control.

**Method output rule:** In XML, output the full **`tool_name:method`** such as **`mouse:click_index`**, **`mouse:double_click_index`**, **`mouse:hover_at`**. Do **not** output bare **`mouse`** unless your runtime explicitly puts `method` in `tool_args`.

**Call priority:** Prefer **composite_action**, **hotkey**, or **modified_click** when one call achieves the same goal with fewer steps.

**Overlay-index methods** (require `index`, `goal`): `click_index`, `double_click_index`, `right_click_index`, `hover_index`.

**Coordinate methods** (require `goal`, `x`, `y`): `click_at`, `double_click_at`, `right_click_at`, `hover_at`.

**Drag (left button down → move → up):**
- **`drag_from_to_at`** — `goal`, **`x1`**, **`y1`** (press here), **`x2`**, **`y2`** (release here). Normalized coordinates, **same scale** as **`click_at`** (current inject **Required range** / reference bbox).
- **`drag_from_to_index`** — `goal`, **`from_index`**, **`to_index`** (overlay centers on **this** annotated frame). Use for sliders, reorder handles, range selection by dragging between two labeled regions.

Optional **`human_like`** (bool) on both — same meaning as other mouse methods (smoothed move to start and eased drag).

**Current cursor (no move):** **`click_current`**, **`double_click_current`**, **`right_click_current`** — only **`goal`**. Does **not** move the pointer; use when the cursor is **already** on the target (**POINTER** / prior step). Prefer index or coordinate methods when you need to aim from the screenshot.

**Other:** **`scroll_at_current`** (`goal`, `lines`) — use when the mouse is already inside the scrollable area (no overlay pick, no typed x,y). **`lines`** is an approximate **line count** to move the viewport (positive = up, negative = down), **not** a 1–10 “strength” knob.

**Choosing `lines`:** Estimate the visible rows in the scrollable region and choose a signed line count that gives the right overlap. The runtime does **not** secretly rescale your value. Avoid tiny values unless you truly need a micro-nudge.

**Mandatory reminder after any scroll tool call:** On the next turn, judge movement with **CUR_SCREEN** vs **PREV_SCREEN** only. **No visible change** means **scroll failed**; change anchor or tactic and do **not** use `screen_reader:extract` until the viewport moves.

**Offset move:** **`move_offset`** (`goal`, `dx`, `dy`) — move the cursor by **dx**, **dy** pixels from its **current** position (right/down positive). Optional **`human_like`** (bool). Use for small aim corrections without picking an overlay index or normalized x,y. Large moves are clamped (see runtime).

Parameter constraints:
- **`goal`** is required for all methods. Phrase it as the intended visible outcome, not the bare click. If the target is text, include the exact visible text; otherwise give a brief visual description. See **Communication** → **Action policy**.
- **`action`** is required. See **Communication** → **Action description in tool_args**.
- For **coordinate methods**: `x`, `y` must match the **scale in the current screen inject** (mouse line + **Required range** + reference bbox numbers for that turn). Derive from a reference with explicit coordinates; do not guess or use a scale from memory of an older turn.

Scroll workflow: When the mouse is already in the scrollable area, use `scroll_at_current` directly. When you need to target a specific region first, use **composite_action:scroll_at_index** (overlay-index), then **mouse:scroll_at_current** for further scrolls.
