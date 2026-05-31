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
  required:
    - method
    - goal
  additionalProperties: true
---

### composite_action

Use for typing and indexed scroll actions.

Primary may use both index and coordinate methods. Pick one route per call from tier communication.

Methods:
- `composite_action.type_text_at_index`
- `composite_action.type_text_at`
- `composite_action.type_text_at_focused`
- `composite_action.scroll_at_index`

Parameter constraints:
- `goal` and `action` are required.
- `text` is required for type methods.
- `auto_enter` defaults to false.
- Use either index args or coordinate args in one call; do not mix.

Optional `wait` in `tool_args`: 1-5 seconds.
