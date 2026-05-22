### modified_click

Use for **modifier+click** multi-select: **Cmd/Ctrl+click** (add items) or **Shift+click** (range).

**This session uses overlay index methods only.** **Forbidden:** `modified_click_at` and coordinate **`positions`**.

**Call priority:** Prefer **modified_click** when selecting multiple items in one call.

**Index method:**
- **`modified_click:modified_click_index`** (`goal`, `action`, `indices`, optional `range_select`) — **`indices`**: overlay numbers on the current annotated frame. **`range_select`**: true with exactly two indices for Shift+range; false for Cmd/Ctrl+add.

Parameter constraints:
- **`goal`** and **`action`** are required.
- Each index must appear on the current annotated screenshot.

**Optional `wait` in `tool_args`:** **~2–3** s for selection highlight to settle.
