---
schema:
  type: object
  properties:
    method:
      type: string
      enum:
        - click
        - double_click
        - right_click
        - hover
        - drag_from_to
    goal:
      type: string
    action:
      type: string
    x:
      type: number
    y:
      type: number
    x1:
      type: number
    y1:
      type: number
    x2:
      type: number
    y2:
      type: number
    human_like:
      type: boolean
    wait:
      type: number
  required:
    - method
    - goal
  additionalProperties: true
---

### mouse_at

Single mouse action using session-normalized coordinates (0-1000).

Use session x/y from `[CUR_SCREEN]` Overlay reference bboxes for precise positioning.

Methods:
- `mouse_at.click` — click at (x, y)
- `mouse_at.double_click` — double-click at (x, y)
- `mouse_at.right_click` — right-click at (x, y)
- `mouse_at.hover` — hover at (x, y)
- `mouse_at.drag_from_to` — drag from (x1, y1) to (x2, y2)

Parameter constraints:
- `goal` and `action` are required.
- `click`, `double_click`, `right_click`, `hover` need `x` and `y`.
- `drag_from_to` needs `x1`, `y1`, `x2`, `y2`.

Optional `wait` in `tool_args`: 1-5 seconds.
