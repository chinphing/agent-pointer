### modified_click

Use for **modifier+click** multi-select: **Cmd/Ctrl+click** (add items) or **Shift+click** (range).

Use coordinate methods as the default route in this guide.

**Call priority:** Prefer **modified_click** for multi-select in one call.

**Coordinate method:**
- **`modified_click:modified_click_at`** (`goal`, `action`, `positions`, optional `range_select`) — **`positions`**: **`[[x,y], …]`** from **Location** / **`Overlay reference bboxes`**. **`range_select`**: true with exactly two positions for Shift+range.

Parameter constraints:
- **`goal`** and **`action`** are required.
- Each **`(x,y)`** must trace to **`Overlay reference bboxes`** this turn.

**Optional `wait` in `tool_args`:** **~2–3** s for selection highlight to settle.
