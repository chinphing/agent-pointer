# Task board v2 schema (maintainer)

Runtime prompts: `crates/pointer-core/src/task_board/prompts/task_board.md` (English).

## Document

| Field | Type | Notes |
|-------|------|-------|
| `version` | number | `2` |
| `task_id` | string | `tb_{store_key}` |
| `meta` | object | `goal`, `status`, `step_count`, `max_steps`, optional `expected_total`, `scope`, `root_target`, `parent_sub_task_id` |
| `global_context` | object | `key_findings[]`, `artifacts` |
| `board` | array | Milestone / local rows |

## Row

| Field | Required | Notes |
|-------|----------|-------|
| `id` | yes | Stable; child locals often `local_*` |
| `title` | yes | |
| `status` | yes | See prompt |
| `depends_on` | no | Prerequisite row ids |
| `retry_count` | no | `>= 2` may set `reflection_required` in tool result |
| `output` | no | Evidence summary when `done` |
| `detailed_plan` | no | Optional long plan text for in-flight rows; host clears on first `done` transition |
| `verification` | no | One-line proof contract |
| `blockedBy` | no | |

## Methods

`init`, `replace`, `patch`, `prune`, `finalize`, `sync_finding` (child → parent findings), `check_deps` (read-only).

### Coverage contract (`expected_total`)

- Use `expected_total` for exhaustive matrix/combinational tasks.
- Set it in `task_board:init` (or `meta.expected_total`).
- When set, `init`/`replace` requires row count to match it exactly.
- `patch` is still incremental row updates.

## Host-injected helper args (not model-authored)

- `_conversation_id`: trusted store-key binding.
- `_recent_action_tools`: recent non-`task_board` action evidence.
- `_recent_verify_report`: whether recent `verify:report` exists.
- `_recent_verify_pass`: whether recent `verify:report` has `action_result=pass`.

`patch` warnings are structured objects (for machine handling), e.g.
`done_without_evidence`, `done_without_verify_pass`,
`interim_drafts_budget_exceeded`.

## Storage

- Memory: `TaskBoardStore`
- SQLite: `{app_data}/task_boards.db`, table `task_boards(store_key, document, updated_at_ms)`

## Code layout

`crates/pointer-core/src/task_board/` — see `mod.rs`.
