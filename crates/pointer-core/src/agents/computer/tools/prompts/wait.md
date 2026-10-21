---
schema:
  type: object
  properties:
    goal:
      type: string
    action:
      type: string
    seconds:
      type: number
      minimum: 0
      maximum: 60
  required:
    - goal
    - seconds
  additionalProperties: true
---

### wait

Use when a **delay** is needed (e.g. page loading, upload/download settling,
animation, dialog appearing).

This is the **blocking `wait` tool** — not the optional **`wait`** field inside
**`mouse_*`** / **`hotkey`** / **`input_*`** / **`modified_click_*`**. That field
is a short post-action settle window clamped to **1–5 s**; this tool takes
**0–60 s** for explicit pauses while the workflow is still loading or
transferring.

Use **wait** when you need to pause before the next action; combine with other
tools as needed (e.g. navigate then wait then click).

Loading / transfer guidance:
- If there is visible progress (spinner/progress bar/status text changing), do
  not mark the step failed yet.
- Wait, then verify again at a completion surface (download list, transfer
  history, or final file presence indicator).
- Only mark done when completion evidence appears; otherwise keep the step in
  non-terminal verify state (for example `action_result=pending`) and continue
  polling.
