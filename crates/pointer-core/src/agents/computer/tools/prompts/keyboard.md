### key_press / key_down / key_up / keyboard_type

Each tool name is the complete operation — no `method` parameter.

None of these tools click or move the pointer: they act on the **currently
focused** widget. If the target field is not focused yet, focus it first
(**`input_at`** clicks and types in one call) or move focus with a shortcut.

**One key**

- **`key_press`** — Requires **`key`**. Presses and releases one key
  (e.g. `enter`, `tab`, `escape`, `down`, `space`).
- **`key_down`** — Requires **`key`**. Holds the key down.
- **`key_up`** — Requires **`key`**. Releases the key.

Use **`key_down`** … **`key_up`** only for gestures that must span more than one
call (hold `shift` while clicking, hold an arrow key to repeat). A held key
stays held until released — always release it on a later turn. For a one-shot
combination, use **`hotkey`** instead: it presses every key and releases them in
reverse in a single call.

**Text**

- **`keyboard_type`** — Requires **`text`**. Types the text into the focused
  widget with raw key events (no clipboard, no click).

**Tool choice**

- Field must be clicked **and** typed this turn → **`input_at`** (one call).
- Field already focused, clipboard paste is acceptable → **`input_focused`**.
- Field already focused and paste must be avoided (terminal TUI, apps that
  ignore paste) → **`keyboard_type`**.
- Shortcut combination (save, copy, switch tab) → **`hotkey`**.
- Single navigation / submit key → **`key_press`**.

**After the call:** do not assume it worked. Verify on the next screenshot
(text present, dialog opened, focus moved).
