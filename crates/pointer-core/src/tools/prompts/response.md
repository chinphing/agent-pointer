### response

**Never reply in plain chat prose.** Every assistant turn — including final answers — is **one JSON object**. User-visible text goes only in **`tool_args.text`** when **`tool_name`** is **`response`**.

**Shared tool-call envelope (all tools):** Emit **one JSON object** per turn when using tools or a structured final turn—no Markdown fences around the model’s actual output, no extra prose outside the object. Top-level keys: **`thoughts`**, **`headline`**, optional **`sidecar_tools`** (array), then **exactly one** root **`tool_name`** (string) plus **`tool_args`** (object). Under `tool_args`, use **one JSON property per argument**; names must match that tool’s parameters as described for that tool. All string values must be valid JSON strings (escape quotes and newlines).

**Optional `sidecar_tools`:** Zero or more management calls that must appear **only** in this array. Each entry is an object with **`tool_name`** and **`tool_args`**, same shape as a normal single-tool invocation. For each sidecar tool, use **`tool:method`** in **`tool_name`** exactly as in that tool’s doc (e.g. **`task_board:patch`**, **`task_board:replace`**). Do **not** put **`terminal`**, **`file`**, or other **Regular tools** here. If the array is present, run **every** sidecar object **first** in order, then the **root** primary tool.

**Root primary tool:** Exactly **one** root **`tool_name`** / **`tool_args`** after optional sidecars—your **main** action this turn (**`terminal`**, **`file`**, desktop tools, or **`response`**). When **`sidecar_tools`** is present, do **not** duplicate a sidecar-only tool at the root (e.g. do not put **`task_board`** both inside the array and as the root **`tool_name`**).

**`tool_name`:** For multi-behavior tools, use **`tool:method`** (e.g. **`file:read`**, **`mouse:click_index`**, **`composite_action:type_text_at_index`**). For single-behavior tools, use the base name only (**`hotkey`**, **`terminal`**, **`wait`**, **`response`** — not `response:response`). Follow each tool’s description if it specifies an exception.

For other tools, the same envelope applies with the appropriate `tool_name` and `tool_args`; each tool’s description covers parameters and any additional examples.

When **`tool_name` is `response`**, you are delivering the final user-visible reply for this turn; use **`text`** inside **`tool_args`** for the full message body shown in the chat. Call **`response`** only after any other tool calls you intend for this turn are complete, or when no tools are needed.

#### Parameters

- **`text`** (required) — Full answer or result shown to the user.

#### JSON example (`response`)

```json
{
  "thoughts": "Concise summary of reasoning for this step.",
  "headline": "Short headline for the response",
  "tool_name": "response",
  "tool_args": {
    "text": "Full answer or result to the user."
  }
}
```

#### JSON example (sidecar `task_board` + root `terminal`)

```json
{
  "thoughts": "Update board, then run tests.",
  "headline": "Tests",
  "sidecar_tools": [
    {
      "tool_name": "task_board:patch",
      "tool_args": {
        "items": "[{\"id\":\"1\",\"title\":\"Run tests\",\"status\":\"in_progress\",\"verification\":\"cargo test -p foo\"}]"
      }
    }
  ],
  "tool_name": "terminal",
  "tool_args": {
    "command": "cargo test -p foo"
  }
}
```

**tips**
Use `include` patterns from the runtime when you must pull prior tool output verbatim; avoid rewriting large prior results when inclusion is available.
Never rewrite subordinate agent responses in full.
