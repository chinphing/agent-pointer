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
    background:
      type: boolean
  required:
    - agentId
    - goal
  additionalProperties: true
---

### `run_subagent`

Hand off one **self-contained task** to a registered worker or a **self fork**.

**What the parent receives**

- Default (foreground): **`content`** — Markdown: the worker’s final assistant message.
  Merge into your plan; do not paste the full handoff to the user.
- **`background: true`**: `{ jobId, status: "running" }` immediately.
  Use **`job.await`** when this turn needs results.
  If this turn can end, do not tell the user everything is finished.
- Only **`content`** (or the job handle). Worker thinking is not included.

**Rules**

- **`goal`** must stand alone — the worker does **not** see the main chat.
- One tool call represents one task.
- Use **`agentId: "self"`** to fork the current agent for an independent task.
- Self forks are leaf workers and cannot call **`run_subagent`**.
- Registered delegation stops at **`maxSubAgentSpawnDepth`** (default 2).
- Self leaf forks remain allowed at that depth.
- Workers finish with **Markdown** in final assistant **`content`** (no tools on that turn).
- Optional **`taskId`** is for **explicitly continuing the same logical task**: reuse it only when a later handoff genuinely continues the same task (e.g. retry or follow-up on the same goal). For a **new** logical task, omit `taskId` so the host assigns a fresh id — do **not** copy a `taskId` seen in a previous completed result.

**Parallel wave (`self` and `explore`)**

- Same turn, multiple **`agentId: "self"`** and/or **`agentId: "explore"`**
  → may run concurrently in one wave.
- **`coder`** / **`computer`** stay serial (writers / desktop).
- Concurrent only when **all** are true:
  - independent (no wait-on result)
  - no shared mutable state / overlapping writes
  - no user-interactive or desktop-control work
- Otherwise: one call, or sequential turns.

**Background (`self` and `explore` only)**

- Omit / `false` = wait until the worker finishes (default).
- `true` = return `jobId` now; the worker keeps running.
- `coder` / `computer` must stay foreground.
- Need a result this turn → `job.await`.
- Task list is already complete → spawn them all (`background: true`),
  then `job.await` `mode=all`. Host queues to the concurrency cap.
- Next task depends on a finished result → `job.await` `mode=any`,
  then spawn the next one.

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
- If you are in a read-only inspection streak with no edit list, prefer **`run_subagent`** (explore) over another local read round.

**`coder` (general lead only)**

**Sole source** for when the general lead stays local vs delegates to **`coder`** :

- **Delegate directly — no user consent.**
- **Delegate for:** **changing code** (any file count), **changing skill
  prompts** / skill files (any file count), all writes under
  **`~/.pointer/skills/`**, tests, multi-file refactors, and **broad**
  search/mapping (unknown paths, multi-hop discovery).
- **Stay local on the general lead:** known-path **`file_*`** when the path
  is already known — reads (**`file_read`**, scoped **`file_grep`** /
  **`file_list`** / **`file_glob`**) and small **non-code** edits
  (**`file_edit`** / **`file_write`**, about **1–3** plain files:
  notes, user text, simple deliverables).
  Do **not** start a repo-wide scout on the lead thread.
- Loading / installing skills is **not** a coder task —
  use **`skill_read`** / **`skill_import`** when those tools are granted.
- When delegating: next tool must be **`run_subagent`** — no repo scout via
  **`terminal`** (`cat` / `grep` / `find`).
- User questions and facts → **`context`**; worker maps (**`explore`**), edits, tests.
- **`workspaceRoot`** **required** — skill root, user project path, or conversation workspace.
- Before delegate: **`skill_read`** (`path` required; use `SKILL.md` for instructions)
  only when you need skill instructions;
  otherwise go straight to **`run_subagent(coder)`**.

**`self` fork (general or coder lead)**

- **When:** long main thread, or a sub-phase needs many tool rounds without polluting lead context
  (multi-skill steps, research, attachment pipelines) — and the work stays in the **current agent's**
  domain.
- **When not:** work that belongs to **`coder`** / **`computer`** /
  **`explore`** per the sections above — stay on this brief only for
  in-domain leaf work; simple Q&A stays on the lead.
- Worker is a **leaf** (no nested **`run_subagent`**, no user clarify) — brief must be self-contained.
- Parallel rules: see **Parallel wave** above.

**`computer` (general lead only)**

- **User consent required** before **every** **`run_subagent(computer)`** call.
- **Offer first** — ask whether to operate the user's machine for **this** task.
- **Prior consent does not carry forward.** A yes for an earlier task, an earlier
  turn, or a prior session does **not** authorize a new delegation.
- **Only skip the separate ask** when the user's **current message** explicitly
  authorizes hands-on desktop work for **this same task** (not a vague follow-up
  like "continue", "do it", or "same as before").
- Optional **`computerTarget`** — `self` for Pointer UI, `external` for other apps (default inferred from task).
- **List files:** put the file **`localPath`** or **`pointer-media://…`** ref in **`context`** (**Facts**) — the worker expands targets into **`wi_*`** rows in **`global_milestones`** on **`task_board_init`**.

**Per-worker goal notes** (all follow **`goal` template** above)

**`explore` goals**

- First line: **`Scenario: <id>`** (playbook id from explore metadata).
- Then **`What:`** + **`Done when:`** (optional **`Out of scope:`**).

**`computer` goals**

- **`What:`** + **`Done when:`** (visible check). Login/MFA/QR: user action in **`context`** (**Constraints** or **Facts**).

**`coder` goals**

- Follow **`coder` (general lead only)** above for **`workspaceRoot`**.
- Do **not** send patch hunks or **`oldString`/`newString`** — put edit ideas in **Lead suggestion (non-binding)**.

**`self` fork goals**

- **`What:`** + **`Done when:`** (research, skill procedure, attachments, or implementation slice).
- Cannot spawn workers — note in **`context`** (**Constraints**) if the subtask needs **`coder`** /
  **`computer`** / **`explore`** instead.
- Parallel rules: see **Parallel wave** above.

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

- **`agentId`** (required) — Worker id from the
  **delegatable sub-agents** metadata block, or reserved id **`self`**.
  **`self`** does not require **`allowAgents`**.
  Naming **your own** id runs as **`self`** — pass **`self`** directly.
- **`goal`** (required) — **`What:`** + **`Done when:`**; optional **`Out of scope:`** (≤3 lines, **≤25 words per line**; see **Goal vs context**).
- **`context`** (optional) — One string; Markdown **`##` blocks** (not JSON). See template above.
- **`title`** (optional) — Short label for traces.
- **`taskId`** (optional) — Reuse only when explicitly continuing the **same logical task**; for new tasks omit it (host assigns a fresh id). Completed results do **not** echo `taskId` back.
- **`workspaceRoot`** (**required** when **`agentId`** is **`coder`**) —
  Absolute directory for the coder worker.
  Optional for a self fork; when present, it overrides the current workspace.
- **`computerTarget`** (optional, **general → `computer`**) — `self` | `external`.
- **`background`** (optional) — `self` / `explore` only. Default false (join).
  `true` returns `jobId` immediately.

**Handoff flow**

1. Provide `agentId`, `goal`, optional `context` / `title` / `taskId`.
2. Worker explores with native tool calls, then writes Markdown as final **`content`**.
3. Parent reads **`content`** only.
