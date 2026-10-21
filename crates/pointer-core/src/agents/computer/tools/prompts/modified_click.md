### modified_click_select_at / modified_click_range_select_at

Each tool name is the complete operation — no `method` parameter.

**Coordinate-based** (screen pixels, origin at the screenshot's top-left):
- **`modified_click_select_at`** — Cmd/Ctrl+click each position. Requires **`positions`** (`{x,y}` / `[x,y]`, one or more).
- **`modified_click_range_select_at`** — Shift+click range between two positions. Requires **`positions`** `[first, last]`.

Coordinates are **logical pixels**, the same width and height as the screenshot
you are looking at — not normalized, not Retina device pixels.
