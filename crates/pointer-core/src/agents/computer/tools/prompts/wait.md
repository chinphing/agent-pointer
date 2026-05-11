### wait

Use when a **delay** is needed (e.g. page loading, animation, dialog appearing).

Method:
- `wait` (`goal`, `seconds`) — Pause for the given number of seconds. `seconds`: 0–60.

Parameter constraints:
- **`goal`** is required. Describe what is being waited for and the expected result (e.g. "Wait for page load", "Wait for dialog to appear"). If waiting for a specific element, describe it: **text** — exact visible text; **other** — brief description of features.
- **`action`** is required. See **Communication** → **Action description in tool_args**.

Use **wait** when you need to pause before the next action; combine with other tools as needed (e.g. navigate then wait then click).

#### XML example

Use the base tool name **`wait`** (not `wait:wait`).

```xml
<response>
  <thoughts>Allow the dialog animation to finish.</thoughts>
  <headline>Wait</headline>
  <tool_name>wait</tool_name>
  <tool_args>
    <goal>Dialog fully visible</goal>
    <action>pause before clicking OK</action>
    <seconds>1.5</seconds>
  </tool_args>
</response>
```
