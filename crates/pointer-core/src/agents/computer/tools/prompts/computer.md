```json
{
  "type": "object",
  "properties": {
    "goal": { "type": "string" },
    "action": { "type": "string" },
    "method": {
      "type": "string",
      "enum": ["screenshot"]
    }
  },
  "required": ["goal", "action", "method"]
}
```

**`computer`** — metadata about the desktop session (not input capture).

- **`screenshot`** — Does **not** capture a new image. Before every model turn the app injects an annotated desktop as a user message (`[CUR_SCREEN]`). Use that image and overlay indices with `mouse` / `composite_action`. Call `screenshot` only if you need an explicit reminder string in the tool result; it does not change what the model sees.
