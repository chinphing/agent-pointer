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
    indices:
      type: array
      items:
        type: integer
    positions:
      type: array
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

### modified_click

Single **modified_click** tool (multi-select / range-select). Set **`method`** or **`action`**.

Every call requires **`goal`** and **`action`**. Optional: `human_like`, `wait` (1–5 s).

- **`select`** with **`indices`** (one or more overlay indexes) or **`positions`** (`{x,y}` / `[x,y]`, one or more) — Cmd/Ctrl+click each (discrete multi-select)
- **`range_select`** with exactly two **`indices`** `[first, last]` or two **`positions`** — Shift+click range between endpoints (not a long index list)
