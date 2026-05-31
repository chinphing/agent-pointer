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
  required:
    - agentId
    - instruction
  additionalProperties: true
---

### `run_subagent`

Hand off a **self-contained sub-task** to another **worker** agent. The worker’s **deliverable** (for **`explore`**, etc.)
is **Markdown** in the tool result’s **`content`** field — **final assistant content** from the worker, not a
**`response`** tool call.

**What the lead receives**

- **`content`** — **Markdown** string: the worker’s full report (final assistant message text). Treat this as the
  canonical handoff document; merge it into your plan or implementation notes.
- **Sibling fields** (ids, names, optional **`reasoning`**) are metadata alongside **`content`**; read them if useful,
  but the reconnaissance body is **Markdown**, not JSON inside **`content`**.
- If your UI or parser wraps the tool reply in JSON, read the **`content`** property’s string value — that string **is**
  the Markdown report.

**When to use**

- A **separate worker profile** fits the work (for **coder** leads, **`explore`** is the default for read-only mapping) and the task can be described **without** relying on the main thread’s message list.
- You can state **goal, scope, inputs, expected output, and how “done” is judged** entirely inside **`instruction`**.
- **Early delegation is fine** — you do not need to exhaust local **`file`** tools first when mapping is still unclear.

**What belongs in `instruction`**

- **Completion criteria:** Spell out what **correctly finished** means—counts, **no duplicates**, uniqueness rules, coverage or quality bars, or other checks the worker can verify before claiming done.
- **User steps:** If the user gave an explicit **sequence of steps**, put that order in **`instruction`** so the worker follows it.
- **Lead context (recommended for `explore`):** The worker does **not** see the main chat. Paste **verified** facts the lead already found: paths, symbols, **negative** search results (“grep X under Y: 0 hits”), **partial** reads, and **Assumptions (unverified)** on their own lines. Optional headings: **Lead context (trusted)** / **Already checked** / **Still unknown**.
- **Reachability / removal / dead-code tasks (recommended for `explore`):** Name **production entry points** to verify (e.g. main handler, stream loop, CLI entry). Ask for **layered** conclusions (compile vs type reuse vs runtime call vs test-only), **call-site** evidence—not imports alone—and **negative greps** for symbol call sites.

**When not to use**

- You already hold **exact** change paths/lines and the next step is **edit**, **test**, or **terminal**—not more mapping.
- The sub-task still needs **ongoing** access to the main chat; the worker only sees **`instruction`** (plus its own system and tools), not the full user conversation.

**`explore` vs local reconnaissance (coder lead)**

- **Default:** use **`agentId` `explore`** for mapping, tracing, and “where / how” questions—**before** a long local **`file`** loop.
- **Local only** when the map is **already tight**: one neighborhood, one symbol, or user-supplied path+line and a single confirm read is enough—see **Routine workflow** step **Explore** and **Delegating to the `explore` worker** in your primary instructions.
- **Cross-directory or cross-module scans:** if reconnaissance may span multiple packages/layers or require repeated grep→read narrowing, choose **`explore`** first.
- **When unsure**, choose **`explore`**; merge its **`content`** Markdown report, then edit here.

**Target workers**

- Use **`agentId`** only for ids that appear in the **delegatable sub-agents** metadata block in your system context. Any other id will fail.

**Rules**

- **`instruction`** must stand alone: prior turns, paths or facts not written there, or implicit context only in the main chat will **not** be available to the worker.
- A worker run **cannot** call **`run_subagent`** again; do not plan nested delegation.
- Workers **cannot** call **`response`**; they finish by writing Markdown as **assistant content** on the final turn.
- Optional **`taskId`** should stay stable if you need the same sidecar board across multiple handoffs to the same logical task.

#### Parameters

- **`agentId`** (required) — Worker id from the delegatable list.
- **`instruction`** (required) — Full task text: goal, scope, inputs, **completion criteria** (what counts as done), and **ordered steps** when the user supplied them.
- **`title`** (optional) — Short label for traces.
- **`taskId`** (optional) — Stable id for sidecar state; omit to let the host assign one.

**Handoff flow**

1. **Tool call arguments** — provide `agentId`, `instruction`, and optional `title` / `taskId`.
2. **Worker run** — the worker uses native tool calls for exploration, then writes the full Markdown digest as **final assistant content** (no tools on that turn).
3. **Tool result** — the host returns JSON with **`content`** set to that Markdown string. The parent reads **`content`** only.
