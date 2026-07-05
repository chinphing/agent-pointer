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

**`goal` — brief template (≤3 lines)**

At most three short lines in this order (omit optional lines). **Start each line with its prefix:**

```text
What: …
Done when: …
Out of scope: …
```

**Line length (each line, including prefixes)**

- **≤25 words** per line (hard cap).
- **One idea per line** — no numbered steps, semicolon chains, or comma-separated lists.
- If a line would exceed the cap, shorten it or move detail to **`context`** (**Facts** / **Lead suggestion**).

- **`What:`** — outcome to deliver.
- **`Done when:`** (required) — verifiable completion (runnable, testable, or explicit handoff shape).
- **`Out of scope:`** (optional) — boundaries; what not to touch.
- **`explore` only:** first line **`Scenario: <id>`** (playbook id); then **`What:`** / **`Done when:`** as above.
- Do **not** put steps, playbooks, diffs, file paths, API params, or patch ideas in **`goal`**.

**`context` — one string, Markdown blocks (not JSON)**

Use **`##` headings**; omit empty blocks. Keep each block short (bullets, not pasted logs or full API dumps).

```markdown
## Facts
Observed facts: paths, errors, API/config shape, reference locations.
For explore: greps/reads already done (**Already checked**) — do not ask the worker to repeat them.

## Constraints
Normative limits the worker must respect: language, platform, security, scope boundaries.
Do not duplicate Facts here — state rules, not observations.

## User-required approach
Steps or format the **user** mandated (worker follows when feasible).

## Lead suggestion (non-binding)
Your implementation ideas; worker may ignore after verifying.

## Still unknown
Unverified assumptions (optional).
```

- You orchestrate; the worker decides steps, tools, and implementation.
- If the user gave **only** a procedure: short **Done when** in **`goal`**; their steps under **User-required approach**.

**`explore` (coder lead)**

- When to delegate vs stay local: **Delegating to the `explore` worker** and **G2** in primary instructions.
- Worker's **description** in **delegatable sub-agents** metadata — read before calling.
- **`goal`** first line: **`Scenario: <id>`**; then **`What:`** + **`Done when:`** (optional **`Out of scope:`**).
- **`context`:** **Facts** (incl. **Already checked** when delegating explore); **Still unknown** when needed.
- If you are in a read-only **`file_*`** streak with no edit list, prefer **`run_subagent`** (explore) over another local read round.

**`coder` (general lead only)**

- **Delegate directly — no user consent.** All repo source work (analysis, edits, tests); skill
  **writes** under **`~/.pointer/skills/`**. Next tool = **`run_subagent`** — no repo scout
  (`file_read`, **`terminal`** grep/find). User facts → **`context`**.
- **`workspaceRoot`** required. Before delegate: **`skill_read`** only, or one **`file_read`**
  on a user-named path.

**`general-worker` (general lead only)**

- **When:** long main thread, or a sub-phase needs many tool rounds without polluting lead context
  (multi-skill steps, research, attachment pipelines) — and the work stays in the **general** domain.
- **When not:** repo/skill-file writes → **`coder`**; desktop/browser → **`computer`**; simple Q&A → stay local.
- Worker is a **leaf** (no nested **`run_subagent`**, no user clarify) — brief must be self-contained.

**`computer` (general lead only)**

- **User consent required** before **`run_subagent`** — offer first; skip only if the user
  already asked you to operate their machine.
- Optional **`computerTarget`** — `self` for Pointer UI, `external` for other apps (default inferred from task).
- **List files (Type2):** put the file **`localPath`** or **`pointer-media://…`** ref in **`context`** (**Facts**) — the worker planner uses it for **`work_items_source`** on init.

**Per-worker goal notes** (all follow **`goal` template** above)

**`explore` goals**

- First line: **`Scenario: <id>`** (playbook id from explore metadata).
- Then **`What:`** + **`Done when:`** (optional **`Out of scope:`**).

**`computer` goals**

- **`What:`** + **`Done when:`** (visible check). Login/MFA/QR: user action in **`context`** (**Constraints** or **Facts**).

**`coder` goals**

- **`workspaceRoot`** (required on **`run_subagent`**) — skill root, user project path, or conversation workspace.
- Do **not** send patch hunks or **`oldString`/`newString`** — put edit ideas in **Lead suggestion (non-binding)**.

**`general-worker` goals**

- **`What:`** + **`Done when:`** (research, skill procedure, attachments).
- Cannot spawn workers — note in **`context`** (**Constraints**) if the subtask needs **`coder`** / **`computer`** instead.

**Examples (`computer`)**

Good:

```json
{
  "agentId": "computer",
  "goal": "What: Open WeChat and confirm the main chat window is visible.\nDone when: WeChat main window is on screen; hand off confirms it is open.",
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
  "goal": "What: Open WeChat and confirm the main chat window is visible.\nDone when: WeChat main window is on screen; hand off confirms it is open.",
  "context": "## User-required approach\nOpen WeChat exactly as the user described in chat (see steps they gave).",
  "computerTarget": "external"
}
```

#### Parameters

- **`agentId`** (required) — Worker id from the **delegatable sub-agents** metadata block.
- **`goal`** (required) — **`What:`** + **`Done when:`**; optional **`Out of scope:`** (≤3 lines, **≤25 words per line**; see **Goal vs context**).
- **`context`** (optional) — One string; Markdown **`##` blocks** (not JSON). See template above.
- **`title`** (optional) — Short label for traces.
- **`taskId`** (optional) — Stable id for sidecar state.
- **`workspaceRoot`** (**required** when **`agentId`** is **`coder`**) — Absolute directory for the coder worker (skill root, user project, or conversation workspace). Host rejects the call if omitted.
- **`computerTarget`** (optional, **general → `computer`**) — `self` | `external`.

**Handoff flow**

1. Provide `agentId`, `goal`, optional `context` / `title` / `taskId`.
2. Worker explores with native tool calls, then writes Markdown as final **`content`**.
3. Parent reads **`content`** only.
