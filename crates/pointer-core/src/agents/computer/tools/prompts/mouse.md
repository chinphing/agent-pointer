### mouse_click_index / mouse_click_at / mouse_double_click_index / mouse_double_click_at / mouse_right_click_index / mouse_right_click_at / mouse_hover_index / mouse_hover_at / mouse_scroll / mouse_drag_from_to_index / mouse_drag_from_to_at

Flat mouse tools — each tool name is the complete operation. No `method` parameter.

Every call requires **`goal`** (outcome) and **`action`** (human-readable target description for the UI).

Optional: `human_like`, `wait` (1–5 s).

**Index-based** (overlay digit — clicks bbox **center**):
- **`mouse_click_index`** — Requires **`index`**.
- **`mouse_double_click_index`** — Requires **`index`**.
- **`mouse_right_click_index`** — Requires **`index`**.
- **`mouse_hover_index`** — Requires **`index`** (move cursor without clicking).
- **`mouse_drag_from_to_index`** — Requires **`from_index`**, **`to_index`**.

**Coordinate-based** (session 0–1000):
- **`mouse_click_at`** — Requires **`x`**, **`y`**.
- **`mouse_double_click_at`** — Requires **`x`**, **`y`**.
- **`mouse_right_click_at`** — Requires **`x`**, **`y`**.
- **`mouse_hover_at`** — Requires **`x`**, **`y`** (move cursor without clicking).
- **`mouse_drag_from_to_at`** — Requires **`x1`**, **`y1`**, **`x2`**, **`y2`**.

**Current cursor:**
- **`mouse_scroll`** — Requires **`lines`** (1–300 or negative). Scrolls at current cursor position.
