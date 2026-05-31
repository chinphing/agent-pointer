---
schema:
  type: object
  properties:
    method:
      type: string
      enum:
        - select
        - range_select
    goal:
      type: string
    action:
      type: string
    positions:
      type: array
    range_select:
      type: boolean
    human_like:
      type: boolean
    wait:
      type: number
  required:
    - method
    - goal
  additionalProperties: true
---

### modified_click_at

Multi-select clicks using session-normalized coordinates.

Methods:
- `modified_click_at.select` — Cmd/Ctrl+click each position in `positions` to add to selection
- `modified_click_at.range_select` — Shift+click range from first to last position in `positions`

Parameter constraints:
- `goal` and `action` are required.
- `select` needs `positions` (array of `{x, y}` objects or `[x, y]` arrays).
- `range_select` needs `positions` with exactly two entries [first, last].

Optional `wait` in `tool_args`: 1-5 seconds.
