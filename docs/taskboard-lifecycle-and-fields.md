# Taskboard Lifecycle And Fields (v3)

## Field semantics

| Field | Patch | Meaning |
| --- | --- | --- |
| `plan` | replace | Execution plan (markdown). |
| `checkpoint` | replace | Coarse position (`cycle=… \| phase=… \| next=…`). |
| `validate_requirement` | replace | Milestone outcome acceptance criteria. |
| `validate_results` | append only | Outcome evidence (markdown snippets). |
| `extract_requirement` | replace | Extraction spec (markdown). |
| `extract_results` | append only | Extracted data (markdown table/list). |

User delivery: assistant **`content`**, not board fields.

## Injection order (current task)

1. `plan`
2. `checkpoint`
3. `validate_requirement`
4. `validate_results` (recent tail)

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
