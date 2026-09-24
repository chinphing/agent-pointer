### mouse_click_index / mouse_click_at / mouse_double_click_index / mouse_double_click_at / mouse_right_click_index / mouse_right_click_at / mouse_hover_index / mouse_hover_at / mouse_scroll_current / mouse_scroll_index / mouse_drag_from_to_index / mouse_drag_from_to_at

Flat mouse tools — each tool name is the complete operation. No `method` parameter.

Every call requires **`goal`** (outcome) and **`action`** (human-readable target description for the UI).

Optional: `wait` (1–5 s).

**Call priority:** Use **`input_index`** / **`input_at`** / **`input_focused`**
when the goal is to type or replace text in a field — they focus, clear, type,
and submit in one call. Use **`mouse_click_*`** only for pure clicks with **no**
typing on that target this turn (buttons, toggles, icons, links, menus).
**Forbidden:** **`mouse_click_*`** to focus an input box when you will type on
the next turn — use **`input_*`** with **`text`** instead.

**Index-based** (overlay digit — clicks bbox **center**):
- **`mouse_click_index`** — Requires **`index`**.
- **`mouse_double_click_index`** — Requires **`index`**.
- **`mouse_right_click_index`** — Requires **`index`**.
- **`mouse_hover_index`** — Requires **`index`** (move cursor without clicking).
- **`mouse_drag_from_to_index`** — Requires **`from_index`**, **`to_index`**.
- **`mouse_scroll_index`** — Requires **`index`**, **`lines`**. See **Scroll** below.

**Coordinate-based** (session 0–1000):
- **`mouse_click_at`** — Requires **`x`**, **`y`**.
- **`mouse_double_click_at`** — Requires **`x`**, **`y`**.
- **`mouse_right_click_at`** — Requires **`x`**, **`y`**.
- **`mouse_hover_at`** — Requires **`x`**, **`y`** (move cursor without clicking).
- **`mouse_drag_from_to_at`** — Requires **`x1`**, **`y1`**, **`x2`**, **`y2`**.
- Optional **`zoom`** (boolean). Set true only when the point is read on
  **`[Zoom pointer after action]`**. Then every coordinate on that call
  is 0–1000 of the crop. The host maps them onto the full screen.
  Omit it for full-screen coordinates and for every **`*_index`** tool.

**Scroll**

If the pointer is already inside the panel that must move, use **`mouse_scroll_current`**.
Otherwise use **`mouse_scroll_index`** and pick an **`index`** inside that panel
(a list row or the panel body, not chrome outside it).

- **`mouse_scroll_index`** — Requires **`index`**, **`lines`**. Moves to index center, then scrolls.
- **`mouse_scroll_current`** — Requires **`lines`**. Scrolls at the current pointer.

**Choosing `lines`:** Signed line count (positive = up, negative = down). Estimate visible rows in **Target region**; runtime does not rescale.

**After scroll:** Judge movement from the next screenshot — no visible change means scroll failed; change anchor or route before retrying.
