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

### composite_action_focused

Type text into the currently focused input field — no positioning needed.

Methods:
- `composite_action_focused.type_text` — type into focused element, optionally clear first, optionally press Enter

Parameter constraints:
- `goal`, `action` are required.
- `text` is required.
- `clear_first` defaults false; `auto_enter` defaults false.

Optional `wait` in `tool_args`: 1-5 seconds.
