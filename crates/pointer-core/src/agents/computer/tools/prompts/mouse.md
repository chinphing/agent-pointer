### mouse_click_at / mouse_double_click_at / mouse_right_click_at / move_to / mouse_scroll_current / mouse_drag_from_to_at / mouse_down / mouse_up

Each tool name is the complete operation — no `method` parameter.

**Call priority:** when the goal is to type or replace text in a field, use
**`input_at`** / **`input_focused`** instead — they focus, clear, type, and
submit in one call. Reserve **`mouse_click_*`** for pure clicks with no typing
on that target this turn (buttons, toggles, icons, links, menus).

**Every pointing tool takes `target_desc` + `bbox` — both, always:**

```json
{"target_desc": "Save button on the right of the toolbar", "bbox": [404, 245, 427, 262], "goal": "<what this step achieves, in the user's language>"}
```

- **`target_desc`** — what you want to act on, in words.
- **`bbox`** — roughly where it is, `[x1, y1, x2, y2]` in **pixels** of the
  screenshot (see **Locating targets**: normalize to decide where, convert to
  pixels for the answer). A loose box is fine: the tool re-grounds inside it and
  clicks the **confirmed** centre, not the centre of your box. A box around the
  wrong area is not fine — grounding cannot find the target outside it.

**`target_desc`** follows **Writing `target_desc`** in your system prompt —
three parts, in one sentence. The screen position is what separates two
same-named elements on a real page; add an anchor only when it still cannot.

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
- **`mouse_scroll_current`** takes `notches_x`/`notches_y` in **wheel notches**,
  plus optional `target_desc` + `bbox`. With the pair, it re-grounds the panel
  from `target_desc` inside that box and moves the pointer to the **confirmed**
  centre — which may sit nowhere near the centre of the box you passed, the same
  way every other pointing tool works. Do that whenever the pointer is not
  already inside the panel; the result line names the point it settled on. With
  no pair it scrolls whatever sits under the pointer right now.
- **`mouse_drag_from_to_at`** takes **two** pairs, because a drag has two ends:
  `from_target_desc` + `from_bbox` (what to grab) and `to_target_desc` +
  `to_bbox` (where to drop). Both ends are grounded separately, so both must be
  described — a bare point is not enough.

  ```json
  {"from_target_desc": "report.pdf on the desktop", "from_bbox": [62, 278, 99, 333],
   "to_target_desc": "Trash", "to_bbox": [615, 648, 646, 704],
   "goal": "<what this drag achieves, in the user's language>"}
  ```

Coordinates are **0–1000 normalized** — see **Locating targets**.

**Button hold / release**

- **`mouse_down`** — Optional **`button`** (default **`left`**). Holds the
  button at the current pointer.
- **`mouse_up`** — Optional **`button`** (default **`left`**). Releases it.

Use **`mouse_down`** … **`mouse_up`** only for gestures that must span more than
one call (canvas panning, marquee, custom multi-step drag). They are **not**
interchangeable with **`mouse_click_at`**, and a held button stays held until
released — always release it on a later turn.

**Scroll** — **`mouse_scroll_current`** requires **`notches_x`** and/or
**`notches_y`** (wheel notches, 1–1000), plus optional **`target_desc`** +
**`bbox`**. Pass the pair whenever the pointer is not already inside the panel:
the tool grounds it, moves the pointer there, then scrolls. Wheel events only
reach the window under the pointer, so scrolling "at the current pointer" from
outside does nothing.

**Choosing `notches_x` / `notches_y`:** Wheel notches, **not pixels**.
**`notches_y` positive = up**, negative = down; **`notches_x` positive = right**,
negative = left. Distance is proportional to the count: **30–50 for a normal
scroll** (≈half a panel); up to **1000** when you deliberately jump past a long
stretch. A big number skips rows you never saw — never use one just to "make
sure" you get far enough.

**After every scroll**, read the result lines:

- **`Scroll scale:`** ends with the next count — `send notches_y=-50`,
  `keep this count`, or `halve it → send notches_y=-25`. **Send exactly that
  number**; do not re-derive it and do not repeat the count it replaced.
- **`fail (at_edge)`** — that direction is exhausted; stop scrolling.
- **`moved 0`** — the pointer was not over a scrollable area; pass
  `target_desc` + `bbox`.
