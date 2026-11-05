---
schema:
  type: object
  properties:
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
    - action
    - keys
  additionalProperties: true
---

### hotkey

Use for keyboard shortcuts (e.g. Copy, Paste, Save, Undo).

**Call priority:** Prefer **input** or **hotkey** when one call achieves the goal. Use **hotkey** when the action is only a key combination (no click or text input).

- **`keys`**: modifier and key names in order — e.g. comma-separated
  `command, c` (macOS) or `ctrl, c` (Windows/Linux).
- **`action` for a shortcut that acts on a visible target** should name that
  target: include the exact visible text, or a brief description of it.

**Copy / Paste workflow**

- After **Copy** (`command,c` / `ctrl,c`) or clicking a copy control: **next
  turn must be `clipboard_read`** — confirm clipboard text before paste or reuse.
  **Do not** send Copy again without reading first.
- After **`clipboard_read`** confirms content: use **Paste** (`command,v` /
  `ctrl,v`) or **`input_focused`** as needed.
- **`action` for Copy hotkey** must be narrow (e.g. "copy selected text to
  clipboard") — not the full paste-or-use workflow in one **`hotkey`** call.
