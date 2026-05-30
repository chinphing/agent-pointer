---
schema:
  type: object
  properties:
    text:
      type: string
  required:
    - text
  additionalProperties: true
---

### response

Use this tool to send the user-visible answer.

The runtime uses native tool calling.
Do not encode tool calls inside plain text.
Choose tools directly from the registry.
Pass arguments in native tool-call arguments.

Call `response` only when you are ready
to present the final user-facing text
for the current turn.

If you still need to call other tools,
call them first and call `response` last.

#### Parameters

- **`text`** (required) — Full message shown to the user.

**Tips**
Use include-style references when available
instead of repeating very large prior outputs.
Keep subordinate agent output summarized,
not copied verbatim unless explicitly needed.
