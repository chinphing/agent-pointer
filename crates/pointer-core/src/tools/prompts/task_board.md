### `task_board`

Maintain a **session-scoped task board** (ordered items with verification hints).

Use when work has **two or more** verifiable milestones, or when the user asks for explicit tracking.

**Qualified `tool_name` (preferred in XML)**

Use the same **`tool:method`** pattern as other multi-behavior tools.

- **`task_board:replace`** — Replace the **entire** board with **`items`**.
  No merge: whatever you send becomes the full list (empty **`items`** clears the board).
- **`task_board:patch`** — Merge by **`id`**: rows with a matching **`id`** are replaced;
  unknown **`id`** values are **appended**. Omit or skip rows with empty **`id`**.

In **`<tool_args>`**, pass **`items`** (and any other fields below). You may also set **`<method>`**
only when **`tool_name`** is the bare base **`task_board`**; if you use **`task_board:patch`**
or **`task_board:replace`**, the method is already encoded in **`tool_name`**.

**Where to call it**

- When the system prompt includes **Sidecar tools**, put **`task_board:…`** only inside
  **`<sidecar_tools>`** as one or more **`<call>`** entries (see the **`response`** tool docs).
- When you only need **one** board update and **no** other tool this round, you may use a **single**
  root **`<tool_name>`** of **`task_board:patch`** or **`task_board:replace`** with **no**
  **`<sidecar_tools>`** block.

**Item fields**

- **`id`** (required) — Stable identifier for the row.
- **`title`** (required) — Short human-readable title.
- **`status`** (required) — One of: **`pending`**, **`in_progress`**, **`done`**, **`cancelled`**.
- **`verification`** (required) — One line: what evidence proves **`done`**
  (for example a **`terminal`** command you will run, or a **`file:read`** path).
- **`blockedBy`** (optional) — Free text when work is blocked.

**Rules**

- Do not mark **`done`** until the **`verification`** evidence is actually satisfied
  (tool output in-thread), or you state an explicit **risk** note if verification cannot be run.
- Keep the board small and milestone-sized (roughly **3–12** items for typical work).

#### XML example (sidecar + terminal)

```xml
<response>
  <thoughts>Update board then run tests.</thoughts>
  <headline>Verify</headline>
  <sidecar_tools>
    <call>
      <tool_name>task_board:patch</tool_name>
      <tool_args>
        <items>[{"id":"1","title":"Run tests","status":"in_progress","verification":"cargo test -p my-crate"}]</items>
      </tool_args>
    </call>
  </sidecar_tools>
  <tool_name>terminal</tool_name>
  <tool_args>
    <command>cargo test -p my-crate</command>
  </tool_args>
</response>
```
