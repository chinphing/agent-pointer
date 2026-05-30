---
schema:
  type: object
  properties:
    method:
      type: string
      enum:
        - init
        - replace
        - patch
        - prune
        - finalize
        - sync_finding
        - check_deps
    goal:
      type: string
    items: {}
    global_context: {}
    ids:
      type: array
      items:
        type: string
    item_id:
      type: string
    finding:
      type: string
    expected_total:
      type: integer
      minimum: 1
    _conversation_id:
      type: string
  additionalProperties: true
---

### `task_board`

Session-scoped **working memory** for multi-step work (v2 document).

For multi-step work, initialize early and keep milestones concise.
Single-step work may skip the board.

**Methods** — call **`task_board`** with **`method`** set to one of:

- **`init`** — Set **`goal`**, optional **`global_context`**, milestone **`items`**.
  Use 3–8 rows for normal work.
  For matrix/combinational goals,
  keep 3–8 rows by grouping cases
  into meaningful milestones.
- **`replace`** — Replace entire **`board`** (empty **`items`** clears).
- **`patch`** — Merge rows by **`id`**; update **`global_context`**.
- **`prune`** — Cancel **`pending`** rows (optional **`ids`** list).
- **`finalize`** — Mark session complete when **every row** is **`done`** or **`cancelled`**. Sets **`meta.status`** to **`completed`**. Row-level **`patch`** to **`done`** alone does **not** finalize the board.
- **`sync_finding`** — **Child agents only:** append one line to parent **`global_context.key_findings`**.
- **`check_deps`** — Read-only: **`item_id`** → **`ready`** or **`blocked`** + reason.

**Tool result shape (compact — authoritative board is in `[TASK_BOARD]` inject)**

- **`ok`**, **`method`**, **`board_len`**
- **`patch`**: **`patched[]`** with `{ id, status }` per row touched this call; optional **`warnings[]`**; **`reflection_required`**
- **`init`**: optional **`goal`**
- **`prune`**: optional **`cancelled[]`**
- **`finalize`**: **`meta_status`**
- **`check_deps`**: **`item_id`**, **`status`**, optional **`reason`**
- **`sync_finding`**: **`findings_count`**

Treat **`[TASK_BOARD]`** in the injected runtime context as the authoritative snapshot.
Do not expect a full **`document`** in tool results.

**Row `status`:** **`pending`**, **`ready`**, **`in_progress`**, **`done`**, **`cancelled`**, **`failed`**

**Rules**

- Treat **`[TASK_BOARD]`** in the injected runtime context as the authoritative **compact** snapshot.
- If **`[TASK_BOARD]`** is empty and the task is multi-step, call **`task_board`** with **`method`: `init`** in the first round.
- **`patch`** should update only the current task id from **`[TASK_BOARD]`**.
- Mark **`done`** only when the current task goal is already achieved in observable evidence.
- If the root tool is a new action to achieve that goal, patch **`in_progress`** this turn (or skip `done`).
- Keep milestones small (roughly **3–12** rows). Use **`local_*`** ids only on **child** boards (sub-agents).
- For matrix/combinational goals,
  prefer milestone grouping over atomic rows.
  A good default is grouping by interaction form
  (for example: slider-trigger flow,
  point-select flow, popup flow).
- For list-like goals, choose granularity by size:
  - if list size <= 8 and each item needs separate acceptance,
    one item can be one milestone;
  - if list size > 8 or items are repetitive,
    group by batch/type/phase into 3–8 milestones.
- Each grouped milestone should state
  explicit coverage in `verification` / `output`
  (which cases are included, pass/fail count,
  and next uncovered slice).
- Keep row text compact; avoid long prose in `title` / `output` / `verification` to reduce prompt tokens.
- **Do not** patch the parent milestone board from a child agent (use **`sync_finding`** or let the host report completion).
- **Finalize:** When **all** rows are **`done`** or **`cancelled`**, call **`task_board`** with **`method`: `finalize`** in the **same turn** as your final user-facing reply (after the last **`patch`**). Do not leave **`meta.status`** at **`running`** when the session goal is complete.
- After **`retry_count >= 2`** on a stuck row, diagnose internally before the next **`patch`**.
- **Computer / desktop profile only** (when **`verify:report`** is allowed):
  use it for sidecar ordering and UI evidence context:
  - First board-init round may omit **`verify:report`**.
  - After init, run **`verify:report`** first, then **`task_board`** with **`method`: `patch`**.
  - Transition into the next milestone only after the current task goal is complete.
  Engineering profiles (e.g. **Coder**) patch from **test/command/file** evidence instead—no **`verify:report`**.

**`items` array**

Pass a JSON **array** of row objects, or a JSON **string** containing that array
(escaped quotes required).

For a **single-row** `patch`, you may also pass row fields at the top level with
**`item_id`** (alias **`id`**) plus **`status`** / **`title`** / **`verification`**
/ etc. — the host normalizes this to one row.

#### Example — initialize board

```json
{
  "function": {
    "name": "task_board",
    "arguments": {
      "method": "init",
      "goal": "Ship feature X",
      "items": [
        { "id": "m1", "title": "Locate code", "status": "pending" }
      ]
    }
  }
}
```

#### Example (goal already met -> done)

**Computer profile** (with **`verify:report`**):

- Native tool call 1: **`verify:report`**
- Native tool call 2: **`task_board`** with **`method`: `patch`**

**Coder / engineering profile** (no **`verify:report`**):

- After tests or commands satisfy **`verification`**, call **`task_board`** with **`method`: `patch`** only:
  - set current row status to `done`
  - include short `verification` and `output`
- Then write assistant user-facing content directly
  (do not call a `response` tool).

Example patch call:

```json
{
  "function": {
    "name": "task_board",
    "arguments": {
      "method": "patch",
      "items": [
        {
          "id": "m1",
          "status": "done",
          "verification": "Tests pass",
          "output": "Handler updated"
        }
      ]
    }
  }
}
```

#### Example — finalize when all rows are terminal

Call after the last row is **`done`** or **`cancelled`**:

```json
{
  "function": {
    "name": "task_board",
    "arguments": {
      "method": "finalize"
    }
  }
}
```
