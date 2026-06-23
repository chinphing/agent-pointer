# Task board schema (maintainer, v3)

Runtime prompts: `crates/pointer-core/src/task_board/prompts/task_board.md` (English).
Lifecycle: `docs/taskboard-lifecycle-and-fields.md`.

## Document

| Field | Type | Notes |
|-------|------|-------|
| `version` | number | `3` only (non-v3 stored JSON is discarded) |
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
| `plan` | no | Execution plan (markdown); cleared on `done` |
| `checkpoint` | no | Coarse position line |
| `validate_requirement` | no | Milestone outcome acceptance criteria |
| `validate_results` | no | Append-only evidence snippets (`string[]`) |
| `extract_requirement` | no | Extraction spec (markdown) |
| `extract_results` | no | Append-only extract snippets (`string[]`) |
| `blocked_by` | no | Host-set dependency hint |

## Methods

`init`, `replace`, `patch`, `prune`, `finalize`, `sync_finding` (child → parent findings), `check_deps` (read-only).

### Coverage contract (`expected_total`)

- Use `expected_total` for exhaustive matrix/combinational tasks **or** enumerated work_item atomic counts.
- Set it in `task_board:init` (or `meta.expected_total`).
- When **no** milestone has `work_item_mode` / `dynamic_quota`: `init`/`replace` requires row count to match `expected_total` exactly (matrix milestones).
- When any milestone uses **work_items**: `expected_total` is the **atomic item count** in `work_items.db` (may differ from `board.len()`).
- `patch` is still incremental row updates.

## Work item fields (board row)

| Field | Notes |
|-------|-------|
| `work_item_mode` | `enumerated` \| `dynamic` |
| `dynamic_quota` | dynamic mode cap |
| `delivery_format` | `xlsx` \| `csv` \| `txt` \| `jsonl` (delivery milestone; P3 export) |

Inline seed on init: `work_items[]` on enumerated milestone (≤200); stored in `work_items.db`, not board JSON.

## Host-injected helper args (not model-authored)

- `_conversation_id`: trusted store-key binding.
- `_recent_action_tools`: recent non-`task_board` action evidence.
- `_recent_verify_report`: whether recent `verify:report` exists.
- `_recent_verify_pass`: whether recent `verify:report` has `action_result=pass`.

`patch` warnings are structured objects (for machine handling), e.g.
`done_without_evidence`, `done_without_verify_pass`,
`interim_drafts_budget_exceeded`.

## Tool result (compact JSON)

Tool handlers return a **compact** body — not the full `document`.
Authoritative board state for the model is injected as **`[TASK_BOARD]`** each round;
UI reads **`task_board_updated`** + store.

| Method | Key fields |
|--------|------------|
| `patch` | `ok`, `method`, `board_len`, `patched[]` (`id`, `status`), `reflection_required`, optional `warnings[]` |
| `init` | `ok`, `method`, `board_len`, optional `goal` |
| `replace` / `prune` / `finalize` | `ok`, `method`, `board_len`; `prune` adds `cancelled[]`; `finalize` adds `meta_status` |
| `check_deps` | `item_id`, `status`, optional `reason` |
| `sync_finding` | `findings_count` |

## Storage

- Memory: `TaskBoardStore`
- SQLite: `{app_data}/task_boards.db`, table `task_boards(store_key, document, updated_at_ms)`

## Code layout

`crates/pointer-core/src/task_board/` — see `mod.rs`.
