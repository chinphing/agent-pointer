---
schema:
  type: object
  properties:
    agentId:
      type: string
    instruction:
      type: string
    title:
      type: string
    taskId:
      type: string
    workspaceRoot:
      type: string
    computerTarget:
      type: string
      enum:
        - self
        - external
  required:
    - agentId
    - instruction
  additionalProperties: true
---

### `run_subagent`

Hand off a **self-contained sub-task** to another **worker** agent.

**What the lead receives**

- **`content`** — **Markdown** string: the worker’s full report (final assistant message text). Merge into your internal plan; do **not** paste full handoff to the user.
- Sibling fields (ids, names, optional **`reasoning`**) are metadata; the reconnaissance body is **`content`**.

**Rules**

- **`instruction`** must stand alone — the worker does **not** see the main chat.
- Workers **cannot** call **`run_subagent`** again (no nested delegation).
- Workers finish with **Markdown** in final assistant **`content`** (no tools on that turn).
- Optional **`taskId`** stays stable across repeated handoffs to the same logical task.

**`explore` (coder lead)**

- When to delegate vs stay local: **Delegating to the `explore` worker** and **G2** in primary instructions.
- Worker's **description** in **delegatable sub-agents** metadata — read before calling.
- **`instruction`** first line: **`Scenario: <id>`**; include goal, scope, completion criteria, and **Lead context (trusted)** / **Already checked** / **Still unknown**.
- If you are in a read-only **`file_*`** streak with no edit list, prefer **`run_subagent`** (explore) over another local read round.

**`coder` / `computer` (general lead only)**

- Fallback delegates — prefer direct answers, **`skill_*`**, **`web_search`** first.
- **`coder`:** optional **`workspaceRoot`** when user gives a project path; else omit (host sandbox).
- **`computer`:** optional **`computerTarget`** — `self` for Pointer UI, `external` for other apps (default inferred from task).

#### Parameters

- **`agentId`** (required) — Worker id from the **delegatable sub-agents** metadata block.
- **`instruction`** (required) — Full task: goal, scope, **completion criteria**, ordered user steps when given, and Lead context for explore.
- **`title`** (optional) — Short label for traces.
- **`taskId`** (optional) — Stable id for sidecar state.
- **`workspaceRoot`** (optional, **general → `coder`**) — Absolute directory for the coder worker.
- **`computerTarget`** (optional, **general → `computer`**) — `self` | `external`.

**Handoff flow**

1. Provide `agentId`, `instruction`, optional `title` / `taskId`.
2. Worker explores with native tool calls, then writes Markdown as final **`content`**.
3. Parent reads **`content`** only.
