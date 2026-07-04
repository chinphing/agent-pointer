---
schema:
  type: object
  properties:
    agentId:
      type: string
    goal:
      type: string
    context:
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
    - goal
  additionalProperties: true
---

### `run_subagent`

Hand off a **self-contained sub-task** to another **worker** agent.

**What the parent receives**

- **`content`** — **Markdown** string: the worker’s full report (final assistant message text). Merge into your internal plan; do **not** paste full handoff to the user.
- Sibling fields (ids, names, optional **`reasoning`**) are metadata; the reconnaissance body is **`content`**.

**Rules**

- **`goal`** must stand alone — the worker does **not** see the main chat.
- Nested delegation is allowed up to **`maxSubAgentSpawnDepth`** (default 2). At max depth, workers are leaves.
- Workers finish with **Markdown** in final assistant **`content`** (no tools on that turn).
- Optional **`taskId`** stays stable across repeated handoffs to the same logical task.

**Goal vs context (all workers)**

Host maps **`goal`** → worker **Assigned task**; **`context`** → **Lead context**.

| | **`goal`** | **`context`** |
| --- | --- | --- |
| **Write** | Outcome + when it counts as done | Facts you observed (paths, errors, constraints) |
| **Do not write** | Steps, playbooks, diffs, verbatim rewrites | Orders to the worker (unless user-sourced — see labels below) |
| **Worker decides** | — | Steps, tools, and implementation |

- You orchestrate; the worker implements.
- Put **how** in **`context`** only, with a label:
  - **`User-required approach:`** — user mandated method (worker follows when feasible).
  - **`Lead suggestion (non-binding):`** — your idea; worker may ignore after verifying.
- Do **not** put user or lead **how** in **`goal`** because it was numbered or detailed.
- If the user gave **only** a procedure, infer a short done check for **`goal`**; put their steps under **`User-required approach:`**.

**`explore` (coder lead)**

- When to delegate vs stay local: **Delegating to the `explore` worker** and **G2** in primary instructions.
- Worker's **description** in **delegatable sub-agents** metadata — read before calling.
- **`goal`** first line: **`Scenario: <id>`**; include scope and completion criteria.
- Put **Lead context (trusted)** / **Already checked** / **Still unknown** in **`context`**.
- If you are in a read-only **`file_*`** streak with no edit list, prefer **`run_subagent`** (explore) over another local read round.

**`coder` (general lead only)**

- Fallback delegate for **non-skill** repo work — prefer direct answers, **`skill_*`**, **`web_search`** first.
- **User Skill files (`~/.pointer/skills/`):** **delegate immediately** on any **write**
  (create / update / packaging — **any size**). Do **not** use **`file_write`** /
  **`file_edit`** on skill paths yourself — **`run_subagent(agentId="coder")`**.
  **Required `workspaceRoot`:** skill root directory —
  `~/.pointer/skills/{skill-name}/` when editing; `~/.pointer/skills/` when creating new.
  Read-only peek (`file_read` / `skill_read`) before delegating is OK. Overrides
  "ask before delegating" and the fallback rule above.
- Optional **`workspaceRoot`** for **non-skill** repo work when user gives a project path; else omit (host sandbox).

**`general-worker` (general lead only)**

- **When:** long main thread, or a sub-phase needs many tool rounds without polluting lead context
  (multi-skill steps, research, attachment pipelines) — and the work stays in the **general** domain.
- **When not:** repo/skill-file writes → **`coder`**; desktop/browser → **`computer`**; simple Q&A → stay local.
- Worker is a **leaf** (no nested **`run_subagent`**, no user clarify) — brief must be self-contained.

**`computer` (general lead only)**

- Fallback delegate — prefer direct answers, **`skill_*`**, **`web_search`** first.
- Optional **`computerTarget`** — `self` for Pointer UI, `external` for other apps (default inferred from task).
- **List files (Type2):** put the file **`localPath`** or **`pointer-media://…`** ref in **`context`** — the worker planner uses it for **`work_items_source`** on init.

**Per-worker goal notes**

**`explore` goals**

- First line: **`Scenario: <id>`** (playbook id from explore metadata).
- Then scope + completion criteria in plain language.

**`computer` goals**

- Outcome + visible done check. Login/MFA/QR: user action in **`context`**; **`goal`** = target state after that.

**`coder` goals**

- **Skill work:** **`workspaceRoot`** = skill root (`~/.pointer/skills/{name}/` or `~/.pointer/skills/` for new).
- Optional **`workspaceRoot`** for other repo tasks when the user gave a project path.
- Do **not** send patch hunks or **`oldString`/`newString`** — use **`Lead suggestion (non-binding):`** if you have edit ideas.

**`general-worker` goals**

- General-domain outcome + done check (research, skill procedure, attachments).
- Cannot spawn workers — if **`coder`** / **`computer`** is needed, say so in handoff for the lead.

**Examples (`computer`)**

Good:

```json
{
  "agentId": "computer",
  "goal": "Open WeChat and confirm the main chat window is visible. Hand off: WeChat is open.",
  "computerTarget": "external"
}
```

Bad (micro-playbook — prescribes **how** instead of **what**):

```json
{
  "goal": "Step 1: find and open the app. Step 2: click inside the window. Step 3: verify it loaded."
}
```

User required a specific path — put it in **`context`**, not **`goal`**:

```json
{
  "agentId": "computer",
  "goal": "Open WeChat and confirm the main chat window is visible. Hand off: WeChat is open.",
  "context": "User-required approach: open WeChat exactly as the user described in chat (see steps they gave).",
  "computerTarget": "external"
}
```

#### Parameters

- **`agentId`** (required) — Worker id from the **delegatable sub-agents** metadata block.
- **`goal`** (required) — **What** + done check (see **Goal vs context**).
- **`context`** (optional) — Observed **facts**; **how** only under **`User-required approach:`** or **`Lead suggestion (non-binding):`**.
- **`title`** (optional) — Short label for traces.
- **`taskId`** (optional) — Stable id for sidecar state.
- **`workspaceRoot`** (optional, **general → `coder`**) — Absolute directory for the coder worker.
  **Required for Skill file writes:** skill root — `~/.pointer/skills/{skill-name}/` (edit) or
  `~/.pointer/skills/` (create). Optional for repo work when the user named a project path.
- **`computerTarget`** (optional, **general → `computer`**) — `self` | `external`.

**Handoff flow**

1. Provide `agentId`, `goal`, optional `context` / `title` / `taskId`.
2. Worker explores with native tool calls, then writes Markdown as final **`content`**.
3. Parent reads **`content`** only.
