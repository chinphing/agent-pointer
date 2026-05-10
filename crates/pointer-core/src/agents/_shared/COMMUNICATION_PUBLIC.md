# Communication (shared)

Per-agent `COMMUNICATION.md` (optional) and `AGENT.md` body are appended after this block.

## Rules

- Output **one** `<response>...</response>` per turn when using tools or a final structured turn — no Markdown fences, no extra prose outside it.
- Children: `thoughts`, `headline`, `tool_name`, `tool_args` (one XML child per argument; names match the injected tool schema — **one tool per turn**).
- **`tool_name`:** use **`base:method`** for multi-behavior tools (e.g. `mouse:click_index`). Use **base only** for `wait`, `response`, and similar (`wait`, not `wait:wait`). Authoritative names are in the **Available tools** section of the system prompt.
- **Desktop (computer profile):** Before your turn the host injects `[CUR_SCREEN]` with raw and annotated desktop frames (pointer and text caret may be drawn on them), zoom strips for small UI, and the prior turn’s raw when available; older vision images are stripped. Prefer `mouse` / `composite_action` using overlay indices; frame order and labels are in the `[CUR_SCREEN]` message (`[Current screen raw]`, `[Screen annotated]`, `[Screen zoomed top]`, …).

## Example: `response`

```xml
<response>
  <thoughts>Task complete.</thoughts>
  <headline>Reply</headline>
  <tool_name>response</tool_name>
  <tool_args>
    <text>User-visible answer.</text>
  </tool_args>
</response>
```

## Example: `mouse:click_index`

```xml
<response>
  <thoughts>Target indexed on overlay.</thoughts>
  <headline>Open settings</headline>
  <tool_name>mouse:click_index</tool_name>
  <tool_args>
    <goal>Open settings from toolbar</goal>
    <action>click gray gear icon top-right toolbar</action>
    <index>12</index>
  </tool_args>
</response>
```

Do not: multiple `<tool_name>` or nested `<response>` in one turn.
