---
schema:
  type: object
  properties:
    method:
      type: string
    goal:
      type: string
    action:
      type: string
    indices:
      type: array
      items:
        type: integer
    positions:
      type: array
  required:
    - method
    - goal
  additionalProperties: true
---

### modified_click

Use for multi-select or range-select click operations.

Primary may use both index and coordinate methods. Pick one route per call from tier communication.

Methods:
- `modified_click.modified_click_index`
- `modified_click.modified_click_at`

Parameter constraints:
- `goal` and `action` are required.
- `range_select=true` requires exactly two targets.
- Index route uses `indices`; coordinate route uses `positions`.

Optional `wait` in `tool_args`: 1-5 seconds.
