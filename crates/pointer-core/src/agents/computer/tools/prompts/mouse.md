### mouse_click_at / mouse_double_click_at / mouse_right_click_at / mouse_hover_at / mouse_scroll_current / mouse_drag_from_to_at / mouse_down / mouse_up / mouse_drag_to

Each tool name is the complete operation — no `method` parameter.

**Call priority:** when the goal is to type or replace text in a field, use
**`input_at`** / **`input_focused`** instead — they focus, clear, type, and
submit in one call. Reserve **`mouse_click_*`** for pure clicks with no typing
on that target this turn (buttons, toggles, icons, links, menus).

**Every pointing tool takes `target_desc` + `bbox` — both, always:**

```json
{"target_desc": "工具栏右侧的「保存」按钮", "bbox": [775, 265, 819, 283], "goal": "保存文档"}
```

- **`target_desc`** — what you want to act on, in words.
- **`bbox`** — roughly where it is, `[x1, y1, x2, y2]` in logical pixels.
  **Take it from the `locate_element` result** — that is exactly what its
  `bbox` field is for. If you are estimating it yourself, a loose box is fine:
  the tool re-grounds inside it and clicks the **confirmed** centre, not the
  centre of your box.

**`target_desc` uses the same four parts as `locate_element` descriptions**:
appearance + control type + screen position + anchor, in one sentence.

```
工具栏右侧的蓝色圆角按钮，白色文字「保存」，紧邻「撤销」按钮
```

Name alone (`保存`) matches more than one place on a real page — grounding
will pick whichever it finds and validate it as correct, because it really is a
「保存」, just not yours. The position and the anchor are what separate them.

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
- **`mouse_scroll_current`** takes `dx`/`dy` — it scrolls at the current
  pointer, nothing to ground.
- **`mouse_drag_from_to_at`** takes **two** pairs, because a drag has two ends:
  `from_target_desc` + `from_bbox` (what to grab) and `to_target_desc` +
  `to_bbox` (where to drop). Both ends are grounded separately, so both must be
  described — a bare point is not enough.

  ```json
  {"from_target_desc": "桌面上的报告.pdf", "from_bbox": [120, 300, 190, 360],
   "to_target_desc": "回收站", "to_bbox": [1180, 700, 1240, 760],
   "goal": "把报告拖进回收站"}
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
