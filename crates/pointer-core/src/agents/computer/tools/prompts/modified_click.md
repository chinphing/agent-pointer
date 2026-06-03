### modified_click_select_index / modified_click_range_select_index / modified_click_select_at / modified_click_range_select_at

Flat modified-click tools — each tool name is the complete operation. No `method` parameter.

Every call requires **`goal`** (outcome) and **`action`** (human-readable description for UI).
Optional: `wait` (1–5 s).

**Index-based** (overlay digit):
- **`modified_click_select_index`** — Cmd/Ctrl+click each index. Requires **`indices`** (one or more).
- **`modified_click_range_select_index`** — Shift+click range between two indices. Requires **`indices`** `[first, last]`.

**Coordinate-based** (session 0–1000):
- **`modified_click_select_at`** — Cmd/Ctrl+click each position. Requires **`positions`** (`{x,y}` / `[x,y]`, one or more).
- **`modified_click_range_select_at`** — Shift+click range between two positions. Requires **`positions`** `[first, last]`.
