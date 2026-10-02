# 任务板生命周期与字段（v4）

Schema reference: [`task-board-v2-schema.md`](task-board-v2-schema.md).

## Field semantics

| Field | Patch | Meaning |
| --- | --- | --- |
| `status` | required | Current row state (`in_progress`, `done`, …). Every patch must include it. |
| `plan` | replace | Execution plan (markdown). |
| `constraint` | replace | Row-level constraint; may inherit `meta.constraint`. |
| `done_when` | replace | Milestone outcome acceptance criteria. |
| `remark` | replace | Short outcome note when marking `done` (optional while `in_progress`). |
| `delivery_format` | replace | Only on `g_deliver` (`xlsx`, `csv`, …). |

**Removed in v4:** `progress`, `checkpoint`, `validate_requirement`, `validate_results`, `validate_*_delta`, `extract_*`, row-level `work_item_mode`.

User delivery: assistant **`content`**, not board fields.

## Document layers

| Layer | Field | Type1 | Type2 |
| --- | --- | --- | --- |
| Meta | `goal`, `context`, `constraint`, `done_when` | ✓ | ✓ |
| Task-level | `global_milestones[]` | full flow | fixed `g_plan` / `g_exec` / `g_deliver` |
| Per-item SOP | `item_milestones[]` | — | template with `{field}` placeholders |
| Queue | `work_items` (DB) | — | flat per `store_id` |

## Injection order (`[TASK_BOARD]`)

One ladder per turn (host projection):

1. **## Task** — meta
2. **## All tasks (with status)** — global rows (step), item template rows (queue exec), or `g_deliver` only (queue deliver)
3. **## Current task** (+ **## Current task plan** when plan exists); queue exec may show `exec_progress` / `exec_met`
4. **## Work items** — window + `[WORK_ITEM_FOCUS]` (queue mode)

See [`task-board-unified-milestone-inject.md`](task-board-unified-milestone-inject.md) for projection rules.

**Final user summary:** derive tables and counts from injected **`remark`** on `done` rows and `result_summary` on terminal work_items — not from chat memory.

## Tool argument names

Keep init and patch names distinct:

- **`task_board_init` / `task_board_replace`**: rows in **`global_milestones[]`** (JSON array of objects with `id`, `title`, `status`). `goal` and `global_milestones` are required on init (`minItems: 1`).
- **`task_board_patch`**: one row in **`milestones[]`**. Do not send `global_milestones` on patch.

Type enforcement (schema + host):

- Schema types are `array`, not string. `additionalProperties` is false so undeclared keys are not untyped strings.
- Host **rejects** a quoted JSON string (`got string — do not quote the array`). Empty init after parse is an error (`ok: false`), not `board_len=0`.
- Host **rejects** rows dropped for missing `id`.
- Native-array aliases (`milestones` / `items` / `board` on init) are accepted only when the value is already a JSON array — for tests and unconstrained providers. Constrained decoding should emit **`global_milestones`**.

## Lifecycle binding rules

- A taskboard is bound to a user message (`anchor_message_id` on the main-turn store key).
- Computer history trim keeps that bound user row (not merely the first user in the transcript).
- One conversation can contain multiple parent taskboards.
- An ended taskboard must not be injected into later user turns.

## History trim (maintainer)

- **Trigger:** `init` / `replace` / `finalize`, or `patch` with substantive progress: row `done`, non-empty `remark`, or `work_item_delta`. Details: [`agent-task-board-and-verification.md`](agent-task-board-and-verification.md#task_board-触发的历史截断当前实现).
- **Not a trigger:** `in_progress` only; v3 fields (`progress`, `validate_results`, …).
- Computer keeps anchor user + last 10 messages + latest live `[CUR_SCREEN]`.

Continuation rule (model-driven):

- Turn start: host surfaces the **active unfinished** board in planner/execution context when one exists.
- **Continue same scope:** planner/execution **does not** call **`task_board_init`** — patch the existing board; UI anchor stays on the originating user message.
- **New multi-step scope:** planner/execution calls **`task_board_init`** — host opens a fresh board bound to the **current** user turn.
- **Fresh init supersede:** when `task_board_init` runs while the current main-turn key already has board content, the host opens a fresh key **and auto-abandons** the previous unfinished parent board (`meta.status=failed`, open milestones `cancelled`), emits `task_board_updated` for it, and rebinds that abandoned board to its **original** key anchor so it does not stack as a second running bar on the current turn. The same `abandon` tool path cancels `pending` / `ready` / `in_progress` rows so the curtain does not keep a spinner.
- Do not use user-message keyword heuristics for reuse vs new board.

## Work items vs standalone planner (2026-07)

`computerStandalonePlannerEnabled` controls **only**:

- Whether a **pre-execution planner LLM** runs before Computer's first turn.
- Whether Computer is **blocked** from calling `task_board_init` / `task_board_replace` during execution (`computer_no_exec_init`).

**Work item queue** (`_task_board_work_items_enabled`) is **always on for Computer**, independent of the planner switch:

- Seed / validate / `work_item_claim` work when planner is off; Computer self-inits on the first turn.
- Host always rejects Type2 init without seed (`enumerated`) or `dynamic_quota` (`dynamic`), regardless of planner.

When planner is off, execution prompts include the same Type2 `enumerated` vs `dynamic` decision tree as the planner (`task_board.md`, `sub_agent_hint.rs`).

**Dynamic init (2026-07):** when `work_item_mode=dynamic` and `dynamic_quota=N`, host seeds **N pending placeholder rows** (`#1`…`#N`) on init so the UI list is populated immediately. Execution **`work_item_claim`** fills the next unassigned slot when the runtime target is known.

## Host status advancement

- After every apply: `pending` rows with satisfied deps → `ready`, then **ensure one `in_progress`**.
- **Type1** (linear `m*` / step ladder): if none is `in_progress`, promote the first `ready` (else eligible `pending`). Same after a terminal patch so the next step does not sit forever at `ready`.
- **Type2 loop** (`g_plan` + `wi_*` + `g_deliver`): host advances the next `wi_*` to `in_progress` (bootstrap + after terminal item patch). `g_deliver` is not auto-started until all loop items are terminal.

## UI behavior rules

- Inline panels mount only on the bound user message (`parentBindings`).
  Reused boards (resume / no new init) stay on the original anchor; new inits bind to the new user turn.
- Ended taskboards should render as normal inline panels.
- Ended taskboards should not use sticky scroll behavior.
- Unfinished taskboards may stay sticky to support active execution.
- With「默认收缩执行过程」, collapsed turns still keep **all** task boards (running and terminal); only process/tool rows are hidden.
- Spinner: prefer row `in_progress`; if meta is still running and no `in_progress` exists (stale client doc), treat the first `ready`/`pending` as current (aligned with inject **Current task**). The collapsed summary does **not** spin; only the current step icon in the expanded list does. Default collapsed.
- Messages soft-excluded by task-board trim (`contextState.included=false`) render like normal chat rows in the UI (no exclusion badge). When debug prompt dump is enabled, excluded rows are logged as `context_excluded_messages` before each LLM request.
