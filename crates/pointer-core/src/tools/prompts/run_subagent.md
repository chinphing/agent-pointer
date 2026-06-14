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

- Behavior, Lead context template, and handoff merge rules: see **Delegating to the `explore` worker** in your primary instructions and **Handoff contract** in explore policy.
- **`instruction`** first line: **`Scenario: <id>`**; include goal, scope, completion criteria, and **Lead context (trusted)** / **Already checked** / **Still unknown**.
- Prefer **`explore`** when mapping is unclear; use local **`file`** only when path+line are already known (see **G2** gate).

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
