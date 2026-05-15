### `run_subagent`

Hand off a **self-contained sub-task** to another **worker** agent. You get back a JSON result for this turn’s history; you then decide the next step (e.g. **`response`**, more tools, or another handoff).

**When to use**

- A **separate worker profile** is clearly better (e.g. **review** or another isolated pass) and the work can be described **without** relying on the main thread’s message list.
- You can state **goal, scope, inputs, expected output, and how “done” is judged** entirely inside **`instruction`**.

**What belongs in `instruction`**

- **Completion criteria:** Spell out what **correctly finished** means—counts, **no duplicates**, uniqueness rules, coverage or quality bars, or other checks the worker can verify before claiming done.
- **User steps:** If the user gave an explicit **sequence of steps**, put that order in **`instruction`** so the worker follows it.
- **Lead context (recommended for `explore`):** The worker does **not** see the main chat. Paste **verified** facts the lead already found: paths, symbols, **negative** search results (“grep X under Y: 0 hits”), **partial** reads, and **Assumptions (unverified)** on their own lines. Optional headings: **Lead context (trusted)** / **Already checked** / **Still unknown**.

**When not to use**

- Ordinary implementation or debugging you can do with **`file`** / **`terminal`** in this thread.
- The sub-task still needs **ongoing** access to the main chat; the worker only sees **`instruction`** (plus its own system and tools), not the full user conversation.

**Target workers**

- Use **`agentId`** only for ids that appear in the **delegatable sub-agents** metadata block in your system context. Any other id will fail.

**Rules**

- **`instruction`** must stand alone: prior turns, paths or facts not written there, or implicit context only in the main chat will **not** be available to the worker.
- A worker run **cannot** call **`run_subagent`** again; do not plan nested delegation.
- Optional **`taskId`** should stay stable if you need the same sidecar board across multiple handoffs to the same logical task.

#### Parameters

- **`agentId`** (required) — Worker id from the delegatable list.
- **`instruction`** (required) — Full task text: goal, scope, inputs, **completion criteria** (what counts as done), and **ordered steps** when the user supplied them.
- **`title`** (optional) — Short label for traces.
- **`taskId`** (optional) — Stable id for sidecar state; omit to let the host assign one.

#### JSON example (delegate implementation)

```json
{
  "thoughts": "Coder should implement; isolate the patch request.",
  "headline": "Delegate implementation",
  "tool_name": "run_subagent",
  "tool_args": {
    "agentId": "coder",
    "title": "Add retry helper",
    "instruction": "In the workspace, add exponential backoff around the HTTP client. Keep public API unchanged. Done means: (1) tests for the crate pass; (2) no duplicate retry helpers; (3) at most three new public items. Follow this order: implement, run tests, then summarize risks.",
    "taskId": "retry_http_client"
  }
}
```

#### JSON example (`explore` — read-only reconnaissance)

```json
{
  "thoughts": "Map call chain before edit; isolate noisy search.",
  "headline": "Explore subagent",
  "tool_name": "run_subagent",
  "tool_args": {
    "agentId": "explore",
    "title": "Trace request handler",
    "instruction": "Goal: document how incoming HTTP requests reach the handler that parses JSON tool calls.\n\nScope: server crate only; do not enter UI or bundled assets.\n\nCompletion: (1) Forward trace from public entry to the parser function with path+line each hop; (2) Backward trace from parser to top-level caller; (3) List open questions if any hop is unclear.\n\n---\nLead context (trusted)\n- READ_AT src/server.rs:L40-L120 — saw router registration but not downstream.\n- GREPPED pattern=parse_tool_call hits=3 under server/.\n\nAlready checked\n- grep for `legacy_handler` under server/: 0 hits.\n\nStill unknown\n- Which module registers the stream endpoint.\n",
    "taskId": "explore_http_tool_parse"
  }
}
```
