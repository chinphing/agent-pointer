### wait

Use when a **delay** is needed (e.g. page loading, animation, dialog appearing).

This is the **blocking `wait` tool** — not the optional **`wait`** field inside **`mouse`** / **`hotkey`** / **`composite_action`** / **`modified_click`** **`tool_args`** (screenshot settle after successful calls; 1–5 s).

**`wait:wait`** (`goal`, `seconds`) — Pause for the given number of seconds. `seconds`: 0–60.

Parameter constraints:
- **`goal`** is required. Describe what is being waited for and the expected result (e.g. "Wait for page load", "Wait for dialog to appear"). If waiting for a specific element, describe it: **text** — exact visible text; **other** — brief description of features.
- **`action`** is required. Brief visible step or pause reason (match **`Tool route:`** line **2** wording when applicable).

Use **wait** when you need to pause before the next action; combine with other tools as needed (e.g. navigate then wait then click).

#### JSON example

```json
{
  "thoughts": "Allow the dialog animation to finish.",
  "headline": "Wait",
  "tool_name": "wait",
  "tool_args": {
    "goal": "Dialog fully visible",
    "action": "pause before clicking OK",
    "seconds": "1.5"
  }
}
```
