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
    indices:
      type: array
      items:
        type: integer
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

### modified_click_index

Multi-select clicks using overlay index numbers.

Methods:
- `modified_click_index.select` — Cmd/Ctrl+click each index in `indices` to add to selection
- `modified_click_index.range_select` — Shift+click range from first to last index in `indices`

Parameter constraints:
- `goal` and `action` are required.
- `select` needs `indices` (array of index numbers).
- `range_select` needs `indices` with exactly two items [first, last].

Optional `wait` in `tool_args`: 1-5 seconds.
