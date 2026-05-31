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
    index:
      type: integer
    from_index:
      type: integer
    to_index:
      type: integer
    anchor:
      type: string
    dx:
      type: integer
    dy:
      type: integer
    human_like:
      type: boolean
    wait:
      type: number
  required:
    - method
    - goal
  additionalProperties: true
---

### mouse_index

Single mouse action using overlay index numbers.

Use index numbers from the `[CUR_SCREEN]` annotations — fast and pattern-free.

Methods:
- `mouse_index.click` — left-click element by index
- `mouse_index.double_click` — double-click element by index
- `mouse_index.right_click` — right-click element by index
- `mouse_index.hover` — hover over element by index
- `mouse_index.drag_from_to` — drag from one index to another

Parameter constraints:
- `goal` and `action` are required.
- `click`, `double_click`, `right_click`, `hover` need `index`.
- `drag_from_to` needs `from_index` and `to_index`.

Optional `wait` in `tool_args`: 1-5 seconds.
