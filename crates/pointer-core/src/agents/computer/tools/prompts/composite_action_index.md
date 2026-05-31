---
schema:
  type: object
  properties:
    method:
      type: string
      enum:
        - type_text
        - scroll
    goal:
      type: string
    action:
      type: string
    index:
      type: integer
    text:
      type: string
    lines:
      type: integer
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

### composite_action_index

Type text or scroll at an overlay-indexed element.

Methods:
- `composite_action_index.type_text` — click, clear, type, optionally press Enter
- `composite_action_index.scroll` — scroll at element by index

Parameter constraints:
- `goal` and `action` are required.
- `type_text` needs `index` and `text`. Optional: `clear_first` (default false), `auto_enter` (default false).
- `scroll` needs `index` and `lines` (positive=down, negative=up, 1-300/-300 to -1).
- Do not mix index and coordinate args.

Optional `wait` in `tool_args`: 1-5 seconds.
