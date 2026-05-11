### modified_click

Use for **modifier+click** to multi-select: **Cmd/Ctrl+click** (add non-contiguous items) or **Shift+click** (select a contiguous range).

**Call priority:** Prefer **modified_click** when selecting multiple items in one call instead of multiple single clicks.

**Overlay selection:** For each value in **`indices`**, prefer the **inner** label (number inside that row/control’s box: inner corners or inner top/bottom/left/right mid-edge) when backgrounds are similar.

**Modifier behavior:**
- **Cmd (macOS) / Ctrl (Windows/Linux) + click** — Add each clicked item to the selection (non-contiguous). Use when selecting scattered items (e.g. rows 2, 5, 7). Pass all indices in `indices` (same format as a multi-value or list string accepted by the runtime); do **not** set `range_select`.
- **Shift + click** — Select a contiguous range from first to last. Use when selecting "from item A to item B" (e.g. rows 3 through 8). Pass **only the first and last index** in order as `indices` and set **`range_select`** to true. The tool will click the first item, then Shift+click the last item so the UI selects the range.

**Overlay-index methods:**
- **`modified_click_index`** (`indices`, `goal`, optional `range_select`) — By default holds Cmd/Ctrl and clicks each index (add to selection). If **`range_select`** is true, `indices` must be exactly two numbers (first, then last) in order; the tool clicks first then Shift+clicks last to select the range. Use when **`[Annotated after action]`** shows index numbers on list items, checkboxes, or file picker items.

**Coordinate methods:**
- **`modified_click_at`** (`goal`, `positions`, optional `range_select`) — Same modifier logic by position: a sequence of x/y pairs per the **mouse** tool’s coordinate convention for this turn. For range, pass two positions and set `range_select` to true.

Parameter constraints:
- **`goal`** is required. Describe the **target elements**: if an item is **text**, include the **exact visible text**; if **other** (icon, row, checkbox), give a **brief description of its features**. Then state which items you are selecting and the expected result.
- **`action`** is required. See **Communication** → **Action description in tool_args**.
