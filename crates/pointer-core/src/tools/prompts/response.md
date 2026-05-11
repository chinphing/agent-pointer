### response

**Shared tool-call envelope (all tools):** Emit **one** `<response>...</response>` per turn when using tools or a structured final turn—no Markdown fences around the model’s actual output, no extra prose outside it. Children: `thoughts`, `headline`, `tool_name`, `tool_args`. Under `tool_args`, use **one XML element per argument**; names must match that tool’s parameters as described for that tool. **One tool per turn**—do not emit multiple `tool_name` values or nest `<response>`.

**`tool_name`:** For multi-behavior tools, use **`tool:method`** (e.g. **`file:read`**, **`mouse:click_index`**, **`composite_action:type_text_at_index`**). For single-behavior tools, use the base name only (**`hotkey`**, **`terminal`**, **`wait`**, **`response`** — not `response:response`). Follow each tool’s description if it specifies an exception.

For other tools, the same envelope applies with the appropriate `tool_name` and `tool_args`; each tool’s description covers parameters and any additional examples.

When **`tool_name` is `response`**, you are delivering the final user-visible reply for this turn; use **`text`** for the full message body shown in the chat. Call **`response`** only after any other tool calls you intend for this turn are complete, or when no tools are needed.

#### Parameters

- **`text`** (required) — Full answer or result shown to the user.

#### XML example (`response`)

```xml
<response>
  <thoughts>Brief reasoning for this final answer.</thoughts>
  <headline>Short headline for the response</headline>
  <tool_name>response</tool_name>
  <tool_args>
    <text>Full answer or result to the user.</text>
  </tool_args>
</response>
```

**tips**
Use `include` patterns from the runtime when you must pull prior tool output verbatim; avoid rewriting large prior results when inclusion is available.
Never rewrite subordinate agent responses in full.
