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
        - scroll
        - move_offset
    goal:
      type: string
    action:
      type: string
    lines:
      type: integer
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

### mouse_current

Mouse actions at the current cursor position — no targeting needed.

Use for clicking wherever the cursor already is, scrolling at cursor, or small offset moves.

Methods:
- `mouse_current.click` — left-click at current position
- `mouse_current.double_click` — double-click at current position
- `mouse_current.right_click` — right-click at current position
- `mouse_current.scroll` — scroll at current position (positive `lines`=down, negative=up)
- `mouse_current.move_offset` — move cursor by (dx, dy) pixels from current position

Parameter constraints:
- `goal` and `action` are required.
- `scroll` needs `lines` (1-300 or -300 to -1).
- `move_offset` needs `dx`, `dy` (both within [-8000, 8000]).

Optional `wait` in `tool_args`: 1-5 seconds.
