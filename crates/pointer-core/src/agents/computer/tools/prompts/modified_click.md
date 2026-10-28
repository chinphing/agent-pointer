### modified_click_select_at / modified_click_range_select_at

Each tool name is the complete operation — no `method` parameter.

**Grounded** (0–1000 normalized, origin at the screenshot's top-left):
- **`modified_click_select_at`** — Cmd/Ctrl+click each target. Requires
  **`positions`**: one `{target_desc, bbox}` per target, one or more.
- **`modified_click_range_select_at`** — Shift+click the range between two
  targets. Requires **`positions`** `[first, last]`, each
  `{target_desc, bbox}`.

```json
{"positions": [
   {"target_desc": "first row of the report table", "bbox": [52, 185, 208, 222]},
   {"target_desc": "third row of the report table", "bbox": [52, 278, 208, 315]}
 ],
 "goal": "<what this selection achieves, in the user's language>"}
```

Each position is grounded **before** any click happens, and the whole batch
aborts if one of them cannot be found — a half-applied selection is worse than
none, because the next step would build on a state you did not actually create.
A loose box per target is fine; there is no raw-coordinate form.

**Each `target_desc`** follows **Writing `target_desc`** in your system prompt —
three parts, in one sentence. Two rows of the same table look alike; the screen
position separates them, and an anchor only when it cannot.

**Write each `target_desc` in the language of the UI.** Chinese labels get a
Chinese description — a translated element name does not match what the grounder
reads on screen.

Coordinates are **0–1000 normalized** — see **Locating targets**.
