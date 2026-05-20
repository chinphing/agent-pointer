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
| `dispatch_to_child` | `supervisor.rs` | Init child meta + optional first local row |
| `report_child_status` | `supervisor.rs` after sub-agent | Parent milestone → `done` / `failed` + `output` |
| `sync_global_finding` | `task_board:sync_finding` tool | Child → parent `key_findings` only |
| `check_dependencies` | Supervisor before dispatch | `Ready` / `Blocked` |

`report_child_status` is **not** an LLM tool (avoids races on the parent board).

## Prompt injection

- Child rounds: `[TASK_BOARD]` (local) + `[TASK_BOARD_PARENT]` (read-only), see `sub_agent_prompt.rs`.
- Compact snapshots: `task_board/snapshot.rs`.

## Not in v1 scope

- Parallel sub-agent scheduling
- Pushing all `local_*` rows to the parent board
