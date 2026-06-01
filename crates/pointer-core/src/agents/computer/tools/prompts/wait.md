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
**`mouse_*`** / **`hotkey`** / **`composite_action_*`** / **`modified_click_*`**
**`tool_args`**.

- Optional `tool_args.wait` on those tools is a short post-action settle window
  (clamped to **1–5 s**).
- This `wait` tool is for explicit blocking pauses (**0–60 s**) when the
  workflow is still in a loading or transfer state.

**`wait`** (`goal`, `seconds`) — Pause for the given number of seconds. `seconds`: 0–60.

Parameter constraints:
- **`goal`** is required. Describe what is being waited for and the expected result (e.g. "Wait for page load", "Wait for dialog to appear"). If waiting for a specific element, describe it: **text** — exact visible text; **other** — brief description of features.
- **`action`** is required. Brief visible step or pause reason (match **`Tool route:`** line **2** wording when applicable).

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
