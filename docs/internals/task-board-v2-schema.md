# Task board v2 schema (maintainer)

Runtime prompts: `crates/pointer-core/src/task_board/prompts/task_board.md` (English).

## Document

| Field | Type | Notes |
|-------|------|-------|
| `version` | number | `2` |
| `task_id` | string | `tb_{store_key}` |
| `meta` | object | `goal`, `status`, `step_count`, `max_steps`, optional `scope`, `root_target`, `parent_sub_task_id` |
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
| `verification` | no | One-line proof contract |
| `blockedBy` | no | |

## Methods

`init`, `replace`, `patch`, `prune`, `finalize`, `sync_finding` (child → parent findings), `check_deps` (read-only).

## Storage

- Memory: `TaskBoardStore`
- SQLite: `{app_data}/task_boards.db`, table `task_boards(store_key, document, updated_at_ms)`

## Code layout

`crates/pointer-core/src/task_board/` — see `mod.rs`.
