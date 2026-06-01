---
schema:
  type: object
  properties:
    goal:
      type: string
    action:
      type: string
    keys:
      oneOf:
        - type: array
          items:
            type: string
        - type: string
    wait:
      type: number
      minimum: 1
      maximum: 5
  required:
    - goal
    - action
    - keys
  additionalProperties: true
---

### hotkey

Use for keyboard shortcuts (e.g. Copy, Paste, Save, Undo).

**Call priority:** Prefer **input** or **hotkey** when one call achieves the goal. Use **hotkey** when the action is only a key combination (no click or text input).

Parameters (in `tool_args`):
- **`goal`** (required): Describe the action and expected result. If the shortcut applies to a visible target (e.g. a button or menu), describe that **target element**: **text** — include the exact visible text; **other** — brief description of features (e.g. Save button, folder icon).
- **`keys`** (required): modifier and key names in order—e.g. comma-separated `command, c` (macOS) or `ctrl, c` (Windows/Linux); you may also pass the same sequence as a bracket list string inside one `<keys>` element if you need a single value.
- **`action`** (required): Visible shortcut step (match **`Tool route:`** line **2** wording).

**Optional `wait` in `tool_args`:** After successful calls (1–5 s). Heuristic:
**~1–2 s** for lightweight UI updates; **~3–5 s** when the shortcut triggers
save/export/import, large paste, modal transitions, or page reload.
If completion is still uncertain after this settle wait, use the standalone
`wait` tool and re-verify on a completion surface.
