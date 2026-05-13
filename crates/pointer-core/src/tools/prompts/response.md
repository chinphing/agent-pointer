### response

**Shared tool-call envelope (all tools):** Emit **one** `<response>...</response>` per turn when using tools or a structured final turn—no Markdown fences around the model’s actual output, no extra prose outside it. Children: `thoughts`, `headline`, optional **`sidecar_tools`**, then **exactly one** root pair **`tool_name`** + **`tool_args`**. Under `tool_args`, use **one XML element per argument**; names must match that tool’s parameters as described for that tool.

**Optional `<sidecar_tools>`:** Zero or more management calls that must appear **only** in this block. Each entry is a **`<call>`** with the same **`tool_name` / `tool_args`** shape as a normal single-tool invocation. For each sidecar tool, use **`tool:method`** in **`<tool_name>`** exactly as in that tool’s doc (e.g. **`task_board:patch`**, **`task_board:replace`**). Do **not** put **`terminal`**, **`file`**, or other **Regular tools** here. If the block is present, run **every** sidecar **`call`** first in order, then the **root** primary tool.

**Root primary tool:** Exactly **one** root **`tool_name`** / **`tool_args`** after optional sidecars—your **main** action this turn (**`terminal`**, **`file`**, desktop tools, or **`response`**). When **`<sidecar_tools>`** is present, do **not** duplicate a sidecar-only tool at the root (e.g. do not put **`task_board`** both inside the block and as the root **`tool_name`**).

**`tool_name`:** For multi-behavior tools, use **`tool:method`** (e.g. **`file:read`**, **`mouse:click_index`**, **`composite_action:type_text_at_index`**). For single-behavior tools, use the base name only (**`hotkey`**, **`terminal`**, **`wait`**, **`response`** — not `response:response`). Follow each tool’s description if it specifies an exception.

For other tools, the same envelope applies with the appropriate `tool_name` and `tool_args`; each tool’s description covers parameters and any additional examples.

When **`tool_name` is `response`**, you are delivering the final user-visible reply for this turn; use **`text`** for the full message body shown in the chat. Call **`response`** only after any other tool calls you intend for this turn are complete, or when no tools are needed.

#### Parameters

- **`text`** (required) — Full answer or result shown to the user.

#### XML example (`response`)

```xml
<response>
  <thoughts>Concise summary of reasoning for this step.</thoughts>
  <headline>Short headline for the response</headline>
  <tool_name>response</tool_name>
  <tool_args>
    <text>Full answer or result to the user.</text>
  </tool_args>
</response>
```

#### XML example (sidecar `task_board` + root `terminal`)

```xml
<response>
  <thoughts>Update board, then run tests.</thoughts>
  <headline>Tests</headline>
  <sidecar_tools>
    <call>
      <tool_name>task_board:patch</tool_name>
      <tool_args>
        <items><![CDATA[[{"id":"1","title":"Run tests","status":"in_progress","verification":"cargo test -p foo"}]]]></items>
      </tool_args>
    </call>
  </sidecar_tools>
  <tool_name>terminal</tool_name>
  <tool_args>
    <command><![CDATA[cargo test -p foo]]></command>
  </tool_args>
</response>
```

**tips**
Use `include` patterns from the runtime when you must pull prior tool output verbatim; avoid rewriting large prior results when inclusion is available.
Never rewrite subordinate agent responses in full.
