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
    index:
      type: integer
    from_index:
      type: integer
    to_index:
      type: integer
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
  required:
    - method
    - goal
  additionalProperties: true
---

### mouse

Use for a single mouse action: click, double-click, right-click, hover, drag, scroll at current cursor, or a small offset move.

Primary may use both index and coordinate methods. Pick one route per call from tier communication.

Index methods: `mouse.click_index`, `mouse.double_click_index`, `mouse.right_click_index`, `mouse.hover_index`, `mouse.drag_from_to_index`.
Coordinate methods: `mouse.click_at`, `mouse.double_click_at`, `mouse.right_click_at`, `mouse.hover_at`, `mouse.drag_from_to_at`.
Current cursor methods: `mouse.click_current`, `mouse.double_click_current`, `mouse.right_click_current`, `mouse.scroll_at_current`, `mouse.move_offset`.

Parameter constraints:
- `goal` and `action` are required.
- Use either index args (`index`/`from_index`/`to_index`) or coordinate args (`x/y` or `x1/y1/x2/y2`) in one call.
- Do not mix index and coordinate args in one call.

Optional `wait` in `tool_args`: 1-5 seconds.
