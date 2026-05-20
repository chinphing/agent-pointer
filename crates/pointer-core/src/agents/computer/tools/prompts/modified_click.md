### modified_click

Use for **modifier+click** multi-select: **Cmd/Ctrl+click** (add items) or **Shift+click** (contiguous range).

**Call priority:** Prefer **modified_click** when selecting multiple items in one call instead of many single clicks.

**`*_index` methods are disabled this session.** Use **`modified_click:modified_click_at`** only — compute each **`x`/`y`** via **Location** / **Overlay reference bboxes** inject.

**Coordinate method:**
- **`modified_click:modified_click_at`** (`goal`, `action`, `positions`, optional `range_select`) — Sequence of **`x`/`y`** pairs (same coordinate convention as **`mouse:click_at`**). **Cmd/Ctrl+click** each position when `range_select` is false/absent. **Shift+range:** pass **exactly two** positions (first, last) and set **`range_select`** true.

Parameter constraints:
- **`goal`** and **`action`** are required. Describe target elements and expected selection outcome. See **Communication** → **Action description in tool_args**.
- **`positions`**: array of coordinate pairs — each from **Location** line **3** math, not overlay index numbers.

**Optional `wait` in `tool_args`:** See **Communication** → **Post-action `wait` in `tool_args`**. Heuristic: multi-select often **~2–3** s for highlight to settle.
