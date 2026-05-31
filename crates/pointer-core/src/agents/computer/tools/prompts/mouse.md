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
    lines:
      type: integer
    offset_x:
      type: integer
    offset_y:
      type: integer
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

### mouse

Single **mouse** tool. Set **`method`** to the operation name (`click_index`, `click_at`, …).

Every call requires **`goal`** (outcome) and **`action`** (human-readable target description for the UI).
**`action` is not the operation name** — do not put `click_index` in `action`.

Optional: `human_like`, `wait` (1–5 s).

**Index (overlay digit):**
- **`click_index`**, **`double_click_index`**, **`right_click_index`**, **`hover_index`** — require **`index`** (clicks bbox **center**)
- **`drag_from_to_index`** — requires **`from_index`**, **`to_index`**

**Coordinates (session 0–1000):**
- **`click_at`**, **`double_click_at`**, **`right_click_at`**, **`hover_at`** — require **`x`**, **`y`**
- **`drag_from_to_at`** — requires **`x1`**, **`y1`**, **`x2`**, **`y2`**

**Current cursor:**
- **`click_current`**, **`double_click_current`**, **`right_click_current`**
- **`scroll_at_current`** — requires **`lines`** (1–300 or negative)
- **`move_offset`** — requires **`offset_x`**, **`offset_y`** (pixels from current cursor, ±8000)

Qualified calls (`mouse:click_at`, etc.) set **`method`** automatically.
