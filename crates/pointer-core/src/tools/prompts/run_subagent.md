### `run_subagent`

Hand off a **self-contained sub-task** to another **worker** agent. You get back a JSON result for this turn’s history; you then decide the next step (e.g. **`response`**, more tools, or another handoff).

**When to use**

- A **separate worker profile** is clearly better (e.g. **review** or another isolated pass) and the work can be described **without** relying on the main thread’s message list.
- You can state **goal, scope, inputs, expected output, and how “done” is judged** entirely inside **`instruction`**.

**What belongs in `instruction`**

- **Completion criteria:** Spell out what **correctly finished** means—counts, **no duplicates**, uniqueness rules, coverage or quality bars, or other checks the worker can verify before claiming done.
- **User steps:** If the user gave an explicit **sequence of steps**, put that order in **`instruction`** so the worker follows it.

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

#### JSON example

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
