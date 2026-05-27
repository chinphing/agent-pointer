# Task board: parent / child coordination (maintainer)

## Principle

Details stay on the **child** board (`local_*`). Results are **reported** to the parent milestone. Dependencies are enforced by the **host**. Findings sync via **`global_context.key_findings`**.

## Store keys

| Board | Key |
|-------|-----|
| Parent (lead / supervisor session) | `conversation_id` |
| Child (sub-agent) | `{conversation_id}\u{1f}ptr_sub_agent\u{1f}{task_id}` |

## Gateway (`task_board/gateway/`)

| API | Caller | Purpose |
|-----|--------|---------|
| `sync_parent_board_from_supervisor_plan` | `supervisor.rs` after plan | Upsert parent milestones from planned `AgentTask` rows |
| `dispatch_to_child` | `supervisor.rs` before each sub-agent | Init child meta + seed `local_01` when child board empty |
| `report_child_status` | `supervisor.rs` after sub-agent | Parent milestone → `done` / `failed` + `output` |
| `sync_global_finding` | `task_board:sync_finding` tool | Child → parent `key_findings` only |
| `check_dependencies` | Supervisor before dispatch | `Ready` / `Blocked` |

`report_child_status` is **not** an LLM tool (avoids races on the parent board).

## Prompt injection

- Child rounds: `[TASK_BOARD]` (local) + `[TASK_BOARD_PARENT]` (read-only), see `sub_agent_prompt.rs`.
- Sub-agent session start: `[TASK_BOARD_HINT]` when `task_board` is allowed and the child board is empty (`task_board/sub_agent_hint.rs`).
- Main computer session start: `[TASK_BOARD_HINT]` can also be injected when board is empty (proactive init nudge).
- Compact snapshots: `task_board/snapshot.rs`.

## Observability

Structured logs use prefix **`task_board_obs:`** (`task_board/observability.rs`): `supervisor_plan_sync`, `dispatch_child`, `store_apply`, `snapshot_injected` / `snapshot_skipped_empty`, `sub_agent_init_hint`, `main_agent_init_hint`, `done_soft_validation`. Lines use the global log format from `pointer_core::logging` (local timestamp prefix on every `log` line).

## Soft `done` validation

On `patch` → `done`, the host injects `_recent_action_tools` plus verify signals (`_recent_verify_report`, `_recent_verify_pass`) from recent assistant tool calls. If the row has no `output`/`verification`/action evidence, or if a recent verify report exists but is not pass, the tool result sets `reflection_required` and structured `warnings` entries (soft gate; patch is still accepted).

## Not in v1 scope

- Parallel sub-agent scheduling
- Pushing all `local_*` rows to the parent board
