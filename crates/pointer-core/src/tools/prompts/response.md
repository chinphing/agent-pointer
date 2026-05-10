### response

Final answer to the user. Ends task processing; use only when done or no task is active.

#### Parameters

- **`text`** (required) — Full answer or result shown to the user.

Output format (XML):

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
