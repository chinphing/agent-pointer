# Taskboard Lifecycle And Fields (v4)

Schema reference: [`internals/task-board-v2-schema.md`](internals/task-board-v2-schema.md).

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

1. **## Task** — meta + Type2 mode/total
2. **## Global milestones**
3. **## Current global_milestone**
4. **## Current global_milestone plan** (separate section)
5. **## Item milestones** + **## Current item_milestone** — only while `g_exec` is `in_progress`
6. **## Work items** — window + `[WORK_ITEM_FOCUS]`

While `g_exec` is `done` or `g_deliver` is active, Item blocks are omitted.

**Final user summary:** derive tables and counts from injected **`remark`** on `done` rows and `result_summary` on terminal work_items — not from chat memory.

## Lifecycle binding rules

- A taskboard is bound to a user message (`anchor_message_id` on the main-turn store key).
- Computer history trim keeps that bound user row (not merely the first user in the transcript).
- One conversation can contain multiple parent taskboards.
- An ended taskboard must not be injected into later user turns.

## History trim (maintainer)

- **Trigger:** `init` / `replace` / `finalize`, or `patch` with substantive progress: row `done`, non-empty `remark`, or `work_item_delta`. Details: [`internals/agent-task-board-and-verification.md`](internals/agent-task-board-and-verification.md#task_board-触发的历史截断当前实现).
- **Not a trigger:** `in_progress` only; v3 fields (`progress`, `validate_results`, …).
- Computer keeps anchor user + last 10 messages + latest live `[CUR_SCREEN]`.

Continuation rule:

- If the newest user message clearly expresses continuation intent,
  reuse the latest unfinished taskboard.
- Otherwise create a new taskboard bound to that user message.

## UI behavior rules

- Ended taskboards should render as normal inline panels.
- Ended taskboards should not use sticky scroll behavior.
- Unfinished taskboards may stay sticky to support active execution.
- Messages soft-excluded by task-board trim (`contextState.included=false`) render like normal chat rows in the UI (no exclusion badge). When debug prompt dump is enabled, excluded rows are logged as `context_excluded_messages` before each LLM request.
