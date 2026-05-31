### modified_click

Use for **modifier+click** multi-select: **Cmd/Ctrl+click** (add items) or **Shift+click** (range).

Use overlay indices as the default route in this guide.

**Call priority:** Prefer **modified_click** when selecting multiple items in one call.

**Index method:**
- **`modified_click_index`** (`goal`, `action`, `indices`, optional `range_select`) — **`indices`**: overlay numbers on the current annotated frame. **`range_select`**: true with exactly two indices for Shift+range; false for Cmd/Ctrl+add.

Parameter constraints:
- **`goal`** and **`action`** are required.
- Each index must appear on the current annotated screenshot.

**Optional `wait` in `tool_args`:** **~2–3** s for selection highlight to settle.
