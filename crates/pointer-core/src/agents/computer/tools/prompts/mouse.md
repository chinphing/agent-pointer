### mouse_click_at / mouse_double_click_at / mouse_right_click_at / mouse_hover_at / mouse_scroll_current / mouse_drag_from_to_at / mouse_down / mouse_up / mouse_drag_to

Each tool name is the complete operation — no `method` parameter.

**Call priority:** when the goal is to type or replace text in a field, use
**`input_at`** / **`input_focused`** instead — they focus, clear, type, and
submit in one call. Reserve **`mouse_click_*`** for pure clicks with no typing
on that target this turn (buttons, toggles, icons, links, menus).

**Coordinate-based** (screen pixels, origin at the screenshot's top-left):
- **`mouse_click_at`** — Requires **`x`**, **`y`**. Optional **`button`**
  (`left` / `middle` / `right`, default **`left`**) and **`clicks`** (1 or 2,
  default **1**).
- **`mouse_double_click_at`** — Requires **`x`**, **`y`**.
- **`mouse_right_click_at`** — Requires **`x`**, **`y`**. Compatibility alias
  for **`mouse_click_at`** with `button: right`.
- **`mouse_hover_at`** — Requires **`x`**, **`y`** (move cursor without clicking).
- **`mouse_drag_from_to_at`** — Requires **`x1`**, **`y1`**, **`x2`**, **`y2`**
  (press at the start point).
- **`mouse_drag_to`** — Requires **`x`**, **`y`**. Drags with the left button
  **from wherever the pointer already is** — no start point. Use it only when
  the pointer is already on the item to grab.

**Or describe the target instead of giving coordinates:**

Every tool above except **`mouse_scroll_current`** and
**`mouse_drag_from_to_at`** also accepts **`description`** in place of `x`/`y`:

```json
{"description": "搜索框", "goal": "在搜索框里查行情"}
```

The tool grounds the description against a fresh screenshot and clicks the
returned centre — **one call instead of `locate_element` + `mouse_click_at`**.
Prefer this when you can name the target but do not know its coordinates.

- **Grounding can fail.** If nothing matches, the call returns an error that
  says what was actually there; it will **not** click a guessed position.
  React by re-describing the target (add a distinguishing detail) or by
  scrolling/re-navigating first — do not retry the same description.
- **Grounding costs a few seconds.** When you already have coordinates from a
  `locate_element` call in this same turn, pass `x`/`y` instead — it is
  instant.
- **Do not pass both.** `x`/`y` win; `description` is ignored when coordinates
  are present.

Coordinates are **logical pixels**, the same width and height as the screenshot
you are looking at — not normalized, not Retina device pixels.

**Button hold / release**

- **`mouse_down`** — Optional **`button`** (default **`left`**). Holds the
  button at the current pointer.
- **`mouse_up`** — Optional **`button`** (default **`left`**). Releases it.

Use **`mouse_down`** … **`mouse_up`** only for gestures that must span more than
one call (canvas panning, marquee, custom multi-step drag). They are **not**
interchangeable with **`mouse_click_at`**, and a held button stays held until
released — always release it on a later turn.

**Scroll**

If the pointer is already inside the panel that must move, use **`mouse_scroll_current`**.
Otherwise move the pointer inside that panel first (a list row or the panel body,
not chrome outside it), then scroll.

- **`mouse_scroll_current`** — Requires **`dx`** and/or **`dy`**, in **pixels**.
  Scrolls at the current pointer position.

**Choosing `dx` / `dy`:** Signed pixel deltas. **`dy` positive = up**, negative =
down; **`dx` positive = right**, negative = left. Runtime converts pixels to
wheel notches locally, so small values still move one notch — use a few hundred
pixels for a visible jump, and roughly one screen height for a full page.
At least one axis must be non-zero.

**After scroll:** the `verify:` suffix on `[Recent desktop tool calls]` already
reports whether the content moved. If it says the scroll had no effect, change
the anchor point or route before retrying — do not re-scroll the same spot.
