# Taskboard Lifecycle And Fields (v3)

## Field semantics

| Field | Patch | Meaning |
| --- | --- | --- |
| `status` | required | Current row state (`in_progress`, `done`, …). Every patch must include it. |
| `plan` | replace | Execution plan (markdown). |
| `progress` | replace | Position within milestone (`N/M`, batch label). Legacy `checkpoint` accepted on read. |
| `validate_requirement` | replace | Milestone outcome acceptance criteria. |
| `validate_result_delta` | append | One outcome line per patch while `in_progress`. |
| `validate_results` | replace (`done` only) | Full outcome list when the row completes. |
| `extract_requirement` | replace | Extraction spec (markdown). |
| `extract_result_delta` | append | One extract line per patch while working. |
| `extract_results` | replace (`done` only) | Full extract when the row completes. |

User delivery: assistant **`content`**, not board fields.

## Injection order (current task)

1. `plan`
2. `progress` (replace on patch)
3. `validate_requirement`
4. While `in_progress` / `pending` / `ready`: **`validate_result_delta`** (recent tail only)
5. When current row is `done`: **`validate_results`** (full list, deduped)

## Injection — all tasks list

- `pending` / `in_progress`: `validate_requirement` on the same line.
- `done` / `cancelled` / `failed`: status line, then all deduped `validate_results` snippets on following lines (`  validate_results: …`).

**Final user summary:** agents must derive delivery tables and counts from this injected list — especially **`validate_results`** on **`done`** rows — not from chat memory. Missing evidence → report unverified.

## Lifecycle binding rules

- A taskboard is bound to a user message (`anchor_message_id` on the main-turn store key).
- Computer history trim keeps that bound user row (not merely the first user in the transcript).
- One conversation can contain multiple parent taskboards.
- An ended taskboard must not be injected into later user turns.

## History trim (maintainer)

- **Current:** trim on `init` / `replace` / `finalize` or `patch` with a row `done`; Computer keeps anchor user + last 10 messages + latest live `[CUR_SCREEN]` (soft-exclude). Details: [`internals/agent-task-board-and-verification.md`](internals/agent-task-board-and-verification.md#task_board-触发的历史截断当前实现).
- **Deferred:** trim on board row **`checkpoint` change**, keep only current-round screen inject — documented under **待实现** in the same file; not implemented (risk review pending).

Continuation rule:

- If the newest user message clearly expresses continuation intent,
  reuse the latest unfinished taskboard.
- Otherwise create a new taskboard bound to that user message.

## UI behavior rules

- Ended taskboards should render as normal inline panels.
- Ended taskboards should not use sticky scroll behavior.
- Unfinished taskboards may stay sticky to support active execution.
- Messages soft-excluded by task-board trim (`contextState.included=false`) render like normal chat rows in the UI (no exclusion badge). When debug prompt dump is enabled, excluded rows are logged as `context_excluded_messages` before each LLM request.
