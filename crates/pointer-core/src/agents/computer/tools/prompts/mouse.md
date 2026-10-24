### mouse_click_at / mouse_double_click_at / mouse_right_click_at / mouse_hover_at / mouse_scroll_current / mouse_drag_from_to_at / mouse_down / mouse_up

Each tool name is the complete operation — no `method` parameter.

**Call priority:** when the goal is to type or replace text in a field, use
**`input_at`** / **`input_focused`** instead — they focus, clear, type, and
submit in one call. Reserve **`mouse_click_*`** for pure clicks with no typing
on that target this turn (buttons, toggles, icons, links, menus).

**Every pointing tool takes `target_desc` + `bbox` — both, always:**

```json
{"target_desc": "Save button on the right of the toolbar", "bbox": [775, 265, 819, 283], "goal": "<what this step achieves, in the user's language>"}
```

- **`target_desc`** — what you want to act on, in words.
- **`bbox`** — roughly where it is, `[x1, y1, x2, y2]` in logical pixels. A
  loose box is fine: the tool re-grounds inside it and clicks the **confirmed**
  centre, not the centre of your box. A box around the wrong area is not fine —
  grounding cannot find the target outside it.

**`target_desc`** follows **Writing `target_desc`** in your system prompt —
four parts, in one sentence. The position and the anchor are what separate two
same-named elements on a real page.

**Write `target_desc` in the language of the UI.** Chinese labels get a Chinese
description — a translated element name does not match what the grounder reads
on screen.

The pair is required together because they answer different questions:
`bbox` narrows the search, `target_desc` decides what counts as a match.
Neither alone is enough — a box with no description cannot tell a button from
its label, and a description with no box has to scan the whole screen (slow,
and same-named elements confuse it).

- **Grounding can fail.** If nothing matching `target_desc` is inside the box,
  the call returns an error describing what *was* there; it will **not** click
  a guess. Re-describe with a distinguishing detail, or widen the box, or
  scroll first — do not repeat the same call.
- **No raw-coordinate form exists.** `x`/`y` are gone on purpose: an
  unverified coordinate click is how you end up on the same-named element two
  rows away.
- **`mouse_scroll_current`** takes `dx`/`dy` in **wheel notches**, plus optional
  `target_desc` + `bbox`. With the pair, it grounds the panel and moves the
  pointer there before scrolling — do that whenever the pointer is not already
  inside the panel. Without them it scrolls whatever sits under the pointer
  right now.
- **`mouse_drag_from_to_at`** takes **two** pairs, because a drag has two ends:
  `from_target_desc` + `from_bbox` (what to grab) and `to_target_desc` +
  `to_bbox` (where to drop). Both ends are grounded separately, so both must be
  described — a bare point is not enough.

  ```json
  {"from_target_desc": "report.pdf on the desktop", "from_bbox": [120, 300, 190, 360],
   "to_target_desc": "Trash", "to_bbox": [1180, 700, 1240, 760],
   "goal": "<what this drag achieves, in the user's language>"}
  ```

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

`mouse_scroll_current` takes an optional **`target_desc`** + **`bbox`**. Pass them
when the pointer is not already inside the panel that must move — the tool
grounds the panel, moves the pointer there, then scrolls. Wheel events only
reach the window under the pointer, so scrolling "at the current pointer" from
outside the panel does nothing.

- **`mouse_scroll_current`** — Requires **`dx`** and/or **`dy`**, in **wheel
  notches** (1–100). Add **`target_desc`** + **`bbox`** to scroll a named panel;
  omit them only when the pointer already sits inside it.

**Choosing `dx` / `dy`:** Wheel notches, **not pixels**. **`dy` positive = up**,
negative = down; **`dx` positive = right**, negative = left. A notch is one
detent of a physical wheel — how far that moves depends on the app and its font
size, so there is no pixel equivalent to aim at. Start around **30** for a
meaningful jump. The previous scroll's tool result reports how far it actually
went — `Verify: pass — moved ~0.3 screen` means 30 notches covered a third of
the viewport, so scale from there. At least one axis must be non-zero.

**After scroll:** the previous scroll's tool result reports how far the content
moved, as a fraction of the viewport, and — when nothing moved — whether the
content is already at the edge. `moved 0 screen` with an edge verdict means you
are done; `moved 0 screen` without one means the pointer is not over a
scrollable area — pass `target_desc` + `bbox` to name the panel instead of
adding notches.
