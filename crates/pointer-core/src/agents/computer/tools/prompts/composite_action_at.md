---
schema:
  type: object
  properties:
    method:
      type: string
      enum:
        - type_text
    goal:
      type: string
    action:
      type: string
    x:
      type: number
    y:
      type: number
    text:
      type: string
    clear_first:
      type: boolean
    auto_enter:
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

### composite_action_at

Type text at session-normalized coordinates (0-1000).

Methods:
- `composite_action_at.type_text` — click at (x, y), optionally clear, type text, optionally press Enter

Parameter constraints:
- `goal`, `action` are required.
- `x`, `y`, `text` are required.
- `clear_first` defaults false; `auto_enter` defaults false.

Optional `wait` in `tool_args`: 1-5 seconds.
