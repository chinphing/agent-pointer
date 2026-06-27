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
- Put verified paths, prior grep results, and dependency summaries in **`context`** (optional).
- Nested delegation is allowed up to **`maxSubAgentSpawnDepth`** (default 2). At max depth, workers are leaves.
- Workers finish with **Markdown** in final assistant **`content`** (no tools on that turn).
- Optional **`taskId`** stays stable across repeated handoffs to the same logical task.

**`explore` (coder lead)**

- When to delegate vs stay local: **Delegating to the `explore` worker** and **G2** in primary instructions.
- Worker's **description** in **delegatable sub-agents** metadata — read before calling.
- **`goal`** first line: **`Scenario: <id>`**; include scope and completion criteria.
- Put **Lead context (trusted)** / **Already checked** / **Still unknown** in **`context`**.
- If you are in a read-only **`file_*`** streak with no edit list, prefer **`run_subagent`** (explore) over another local read round.

**`coder` (general lead only)**

- Fallback delegate for **non-skill** work — prefer direct answers, **`skill_*`**, **`web_search`** first.
- **User Skill files (`~/.pointer/skills/`):** **delegate immediately** on any **write**
  (create / update / packaging — **any size**). Do **not** use **`file_write`** /
  **`file_edit`** on skill paths yourself — **`run_subagent(agentId="coder")`**.
  **Required `workspaceRoot`:** skill root directory —
  `~/.pointer/skills/{skill-name}/` when editing; `~/.pointer/skills/` when creating new.
  Read-only peek (`file_read` / `skill_read`) before delegating is OK. Overrides
  "ask before delegating" and the fallback rule above.
- Optional **`workspaceRoot`** for **non-skill** repo work when user gives a project path; else omit (host sandbox).

**`computer` (general lead only)**

- Fallback delegate — prefer direct answers, **`skill_*`**, **`web_search`** first.
- Optional **`computerTarget`** — `self` for Pointer UI, `external` for other apps (default inferred from task).
- **List files (Type2):** put the file **`localPath`** or **`pointer-media://…`** ref in **`context`** — the worker planner uses it for **`work_items_source`** on init.

**Goal authoring (all workers)**

- **`goal`** = **what to achieve** + **how you know it is done** — not a UI playbook.
- Put verified paths, errors, and prior digests in **`context`**, not in **`goal`**.
- Numbered steps in **`goal`** are accepted but **discouraged** — the worker owns **how**; micro-playbooks waste context and can conflict with worker rules.

**When the user explicitly requires an approach**

- Default shape unchanged: **`goal`** = outcome + done check; worker chooses **how**.
- Put the user's required method, steps, tools, or scope limits in **`context`**, labeled **`User-required approach:`** (quote or faithful paraphrase — do not invent extra steps).
- Do **not** move user-required steps into **`goal`** just because they were numbered — keep **what** in **`goal`**, **how** in **`context`**.
- If the user gave **only** a procedure with no clear outcome, infer a short done check for **`goal`** and keep their procedure under **`User-required approach:`**.
- The worker tries the user-required approach when feasible; if it fails or conflicts with worker policy, it reports what was tried in handoff — you may retry or explain to the user.

**`explore` goals**

- First line: **`Scenario: <id>`** (playbook id from explore metadata).
- Then scope + completion criteria in plain language.

**`computer` goals**

- State the **outcome** (e.g. app open, form submitted, setting changed) and **done check** (what must be visible).
- Do **not** prescribe **how** — no navigation paths, click sequences, hotkeys, or tool names; the worker chooses actions from the screen and its own rules.
- Login, MFA, QR, or admin approval: note in **`context`** if the user must act; **`goal`** stays the target state after that.

**`coder` goals**

- Repo outcome + acceptance (tests, files touched, behavior) — not a long file-read script.
- **Skill work:** **`workspaceRoot`** must be the skill root (`~/.pointer/skills/{name}/` or `~/.pointer/skills/` for new).
- Optional **`workspaceRoot`** for other repo tasks when the user gave a project path.

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
- **`goal`** (required) — Outcome + completion criteria (short). No unsolicited step lists — user-required **how** goes in **`context`**.
- **`context`** (optional) — Trusted facts: paths, errors, language, prior task digests.
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
