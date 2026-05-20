### modified_click

Use for **modifier+click** multi-select: **Cmd/Ctrl+click** (add items) or **Shift+click** (contiguous range).

**Call priority:** Prefer **modified_click** when selecting multiple items in one call instead of many single clicks.

**`*_index` methods are disabled this session.** Use **`modified_click:modified_click_at`** only — compute each **`x`/`y`** via **Location** / **Overlay reference bboxes** row **R**.

**Coordinate method:**
- **`modified_click:modified_click_at`** (`goal`, `action`, `positions`, optional `range_select`) — Sequence of **`x`/`y`** pairs (same coordinate convention as **`mouse:click_at`**). **Cmd/Ctrl+click** each position when `range_select` is false/absent. **Shift+range:** pass **exactly two** positions (first, last) and set **`range_select`** true.

Parameter constraints:
- **`goal`** and **`action`** are required. Describe target elements and expected selection outcome (match **`Tool route:`** line **2** wording).
- **`positions`**: array of coordinate pairs — each from **Location** line **3** math, not overlay index numbers.

**Optional `wait` in `tool_args`:** After successful calls (1–5 s). Heuristic: multi-select often **~2–3** s for highlight to settle.
