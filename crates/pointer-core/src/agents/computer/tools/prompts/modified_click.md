### modified_click_select_at / modified_click_range_select_at

Each tool name is the complete operation — no `method` parameter.

**Grounded** (logical pixels, origin at the screenshot's top-left):
- **`modified_click_select_at`** — Cmd/Ctrl+click each target. Requires
  **`positions`**: one `{target_desc, bbox}` per target, one or more.
- **`modified_click_range_select_at`** — Shift+click the range between two
  targets. Requires **`positions`** `[first, last]`, each
  `{target_desc, bbox}`.

```json
{"positions": [
   {"target_desc": "first row of the report table", "bbox": [100, 200, 400, 240]},
   {"target_desc": "third row of the report table", "bbox": [100, 300, 400, 340]}
 ],
 "goal": "select these two rows for comparison"}
```

Each position is grounded **before** any click happens, and the whole batch
aborts if one of them cannot be found — a half-applied selection is worse than
none, because the next step would build on a state you did not actually create.
Take `bbox` from `locate_element`; there is no raw-coordinate form.

**Each `target_desc`** follows **Writing `target_desc`** in your system prompt —
four parts, in one sentence. Two rows of the same table look alike; the position
and the anchor are what separate them.

Coordinates are **logical pixels**, the same width and height as the screenshot
you are looking at — not normalized, not Retina device pixels.
