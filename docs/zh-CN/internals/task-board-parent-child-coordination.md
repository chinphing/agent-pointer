# Task board: parent / child coordination (maintainer)

## Principle

Details stay on the **child** board (`local_*`). Results are **reported** to the parent milestone. Dependencies are enforced by the **host**.

Child boards never write to the parent board. A child's findings reach the lead through its final assistant content, which the host records on the parent milestone via `report_child_status`. The `global_context.key_findings` list is per-board and only written by that board's own `init` / `patch`.

## Store keys

| Board | Key |
|-------|-----|
| Parent (lead session) | `conversation_id` |
| Child (sub-agent) | `{conversation_id}\u{1f}ptr_sub_agent\u{1f}{task_id}` |

## Gateway (`task_board/gateway/`)

| API | Caller | Purpose |
|-----|--------|---------|
| `dispatch_to_child` | before each sub-agent delegation | Init child meta + seed `local_01` when child board empty |
| `report_child_status` | after sub-agent exit | Parent milestone → `done` / `failed` + `output` |
| `check_dependencies` | before dispatch | `Ready` / `Blocked` |

`report_child_status` is **not** an LLM tool (avoids races on the parent board).

## Prompt injection

- Child rounds: `[TASK_BOARD]` (local) + `[TASK_BOARD_PARENT]` (read-only), see `sub_agent_prompt.rs`.
- Lead / sub-agent rounds: empty-board `[TASK_BOARD_HINT]` inject is **disabled** (see `task_board/init_policy.rs`). Live `[TASK_BOARD]` snapshots still inject when the board has content.
- Compact snapshots: `task_board/snapshot.rs`.

## Observability

Structured logs use prefix **`task_board_obs:`** (`task_board/observability.rs`): `dispatch_child`, `store_apply`, `snapshot_injected` / `snapshot_skipped_empty`, `sub_agent_init_hint`, `main_agent_init_hint`, `done_soft_validation`. Lines use the global log format from `pointer_core::logging` (local timestamp prefix on every `log` line).

## Soft `done` validation

On `patch` → `done`, the host injects `_recent_action_tools` plus verify signals (`_recent_verify_report`, `_recent_verify_pass`) from recent assistant tool calls. If the row has no `output`/`verification`/action evidence, or if a recent verify report exists but is not pass, the tool result sets `reflection_required` and structured `warnings` entries (soft gate; patch is still accepted).

## Not in v1 scope

- Parallel sub-agent scheduling
- Pushing all `local_*` rows to the parent board

## Campaign work_items + sub-agents (v4)

When the parent board uses Type2 **`work_items`**, see [`task-board-campaign-work-queue-spec.md`](task-board-campaign-work-queue-spec.md) **§3.6 Parent / child agents + work_items** (historical v1 notes). Runtime: single parent **`store_id`**, flat work_items queue, child claim/report via host Gateway (`gateway/work_item_child.rs`), extended `[TASK_BOARD_PARENT]` assignment fields.
