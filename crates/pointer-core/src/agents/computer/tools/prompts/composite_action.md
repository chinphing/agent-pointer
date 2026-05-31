---
schema:
  type: object
  properties:
    method:
      type: string
    action:
      type: string
    goal:
      type: string
    index:
      type: integer
    x:
      type: number
    y:
      type: number
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
      minimum: 1
      maximum: 5
  required:
    - goal
  additionalProperties: true
---

### composite_action

Single **composite_action** tool (type / scroll / focused typing). Set **`method`** or **`action`**.

Every call requires **`goal`** and **`action`**. Optional: `clear_first`, `auto_enter`, `human_like`, `wait` (1–5 s).

- **`type_text_at_index`**, **`scroll_at_index`** — require **`index`** (bbox **center**); type also needs **`text`**; scroll needs **`lines`**
- **`type_text_at`** — require **`x`**, **`y`**, **`text`** (session 0–1000)
- **`type_text_at_focused`** / focused typing with **`text`** only (no index/x/y)

Do not mix index and coordinate fields in one call.
