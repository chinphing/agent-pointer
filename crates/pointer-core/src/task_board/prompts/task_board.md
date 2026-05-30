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

**Qualified `tool_name`**

- **`task_board:init`** — Set **`goal`**, optional **`global_context`**, milestone **`items`**.
  Use 3–8 rows for normal work.
  For matrix/combinational goals,
  keep 3–8 rows by grouping cases
  into meaningful milestones.
- **`task_board:replace`** — Replace entire **`board`** (empty **`items`** clears).
- **`task_board:patch`** — Merge rows by **`id`**; update **`global_context`**.
- **`task_board:prune`** — Cancel **`pending`** rows (optional **`ids`** list).
- **`task_board:finalize`** — Mark session complete when no incomplete rows remain.
- **`task_board:sync_finding`** — **Child agents only:** append one line to parent **`global_context.key_findings`**.
- **`task_board:check_deps`** — Read-only: **`item_id`** → **`ready`** or **`blocked`** + reason.

Bare **`task_board`** with **`method`** in **`tool_args`** works when not using qualified names.

**Document shape (host returns full `document` in tool result)**

- **`meta`**: **`goal`**, **`status`**, **`step_count`**, **`max_steps`**, optional **`expected_total`**
- **`global_context`**: **`key_findings`**, **`artifacts`**
- **`board[]`**: rows with **`id`**, **`title`**, **`status`**, **`depends_on`**, **`retry_count`**, **`output`**, **`verification`**, **`blockedBy`**

**Row `status`:** **`pending`**, **`ready`**, **`in_progress`**, **`done`**, **`cancelled`**, **`failed`**

**Rules**

- Treat **`[TASK_BOARD]`** in the injected runtime context as the authoritative **compact** snapshot.
- If **`[TASK_BOARD]`** is empty and the task is multi-step, call **`task_board:init`** in the first round.
- `task_board:patch` should update only the current task id from **`[TASK_BOARD]`**.
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
- After **`retry_count >= 2`** on a stuck row, diagnose in **`thoughts`** before the next **`patch`**.
- For workers that emit **`verify:report`**, use it for sidecar ordering and evidence context:
  - First initialization round may omit `verify:report`.
  - After init, run `verify:report` first, then `task_board:patch`.
  - Transition into the next milestone only after the current task goal is complete.

**`items` in `tool_args`**

Pass a JSON **array** of row objects, or a JSON **string** containing that array (escaped quotes required).

#### Example (goal already met -> done)

- Native tool call 1: `verify:report`
- Native tool call 2: `task_board:patch`
  - set current row status to `done`
  - include short `verification` and `output`
- Then write assistant user-facing content directly
  (do not call a `response` tool).
