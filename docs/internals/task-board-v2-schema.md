# Task board schema (maintainer, v4)

Runtime prompts: `crates/pointer-core/src/task_board/prompts/task_board.md` (English).
Lifecycle: `docs/taskboard-lifecycle-and-fields.md`.

## Document

| Field | Type | Notes |
|-------|------|-------|
| `version` | number | `4` (v3 migrates on load) |
| `task_id` | string | `tb_{store_key}` |
| `meta` | object | `goal`, `context`, `constraint`, `done_when`, `work_item_mode`, `dynamic_quota`, `expected_total`, … |
| `global_context` | object | `key_findings[]`, `artifacts` |
| `global_milestones` | array | Task-level rows (serde alias `board`) |
| `item_milestones` | array | Type2 per-item SOP template only |

## Milestone row (`global_milestones` / `item_milestones`)

| Field | Required | Notes |
|-------|----------|-------|
| `id` | yes | Stable; child locals often `local_*` |
| `title` | yes | |
| `status` | yes | See prompt |
| `plan` | no | Execution plan (markdown) |
| `constraint` | no | Row-level; may inherit `meta.constraint` |
| `done_when` | no | Outcome acceptance (was `validate_requirement`) |
| `remark` | no | Short outcome note when `done` (was `validate_results`) |
| `depends_on` | no | Prerequisite row ids |
| `retry_count` | no | `>= 2` may set `reflection_required` |
| `blocked_by` | no | Host-set dependency hint |
| `delivery_format` | no | Only on `g_deliver` (`xlsx`, `csv`, …) |

**Removed in v4:** `progress`, `checkpoint`, `validate_*`, `extract_*`, row-level `work_item_mode`.

## Methods

`init`, `replace` (item_milestones only), `patch`, `prune`, `finalize`, `sync_finding`, `check_deps`.

### Type 1 (no work_items)

- `init`: meta + `global_milestones[]`
- `patch`: `global_milestones` len=1

### Type 2 (work_items)

- `init`: meta + fixed `g_plan`/`g_exec`/`g_deliver` + `item_milestones` + `work_items` seed
- `replace`: **only** full `item_milestones[]`
- `patch`: `milestones` len=1 (item SOP) or `global_milestones` len=1 (global phase); optional `work_item_delta`

## Work items (flat per store)

Stored in `work_items.db` keyed by `store_id` (= conversation store key). No `batch_id`.

Inline seed on init: top-level `work_items[]` (≤50 inline); larger lists via `work_items_source`.

## Host-injected helper args (not model-authored)

- `_conversation_id`: trusted store-key binding.
- `_recent_action_tools`, `_recent_verify_report`, `_recent_verify_pass`

`patch` warnings: `in_progress_without_plan`, `done_without_action`, `g_exec_not_terminal`, `g_deliver_blocked`, `v3_field_rejected`.

## Tool result (compact JSON)

Authoritative state is injected as **`[TASK_BOARD]`** each round.

| Method | Key fields |
|--------|------------|
| `patch` | `ok`, `method`, `board_len`, `patched[]`, `reflection_required`, optional `warnings[]` |
| `init` | `ok`, `method`, `board_len`, optional `goal` |
| `replace` / `prune` / `finalize` | `ok`, `method`, `board_len` |
| `check_deps` | `item_id`, `status`, optional `reason` |
| `sync_finding` | `findings_count` |

## Storage

- Memory: `TaskBoardStore`
- SQLite boards: `{app_data}/task_boards.db`, table `task_boards(store_key, document, updated_at_ms)`
- SQLite work items: `{app_data}/work_items.db`, table `work_items` by `store_id`

## Code layout

`crates/pointer-core/src/task_board/` — see `mod.rs`.
