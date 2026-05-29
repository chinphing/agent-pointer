### `task_board`

Session-scoped **working memory** for multi-step work (v2 document).

For multi-step work, initialize early and keep milestones concise.
Single-step work may skip the board.

**Qualified `tool_name`**

- **`task_board:init`** — Set **`goal`**, optional **`global_context`**, milestone **`items`** (3–8 rows).
- **`task_board:replace`** — Replace entire **`board`** (empty **`items`** clears).
- **`task_board:patch`** — Merge rows by **`id`**; update **`global_context`**.
- **`task_board:prune`** — Cancel **`pending`** rows (optional **`ids`** list).
- **`task_board:finalize`** — Mark session complete when no incomplete rows remain.
- **`task_board:sync_finding`** — **Child agents only:** append one line to parent **`global_context.key_findings`**.
- **`task_board:check_deps`** — Read-only: **`item_id`** → **`ready`** or **`blocked`** + reason.

Bare **`task_board`** with **`method`** in **`tool_args`** works when not using qualified names.

**Document shape (host returns full `document` in tool result)**

- **`meta`**: **`goal`**, **`status`**, **`step_count`**, **`max_steps`**
- **`global_context`**: **`key_findings`**, **`artifacts`**
- **`board[]`**: rows with **`id`**, **`title`**, **`status`**, **`depends_on`**, **`retry_count`**, **`output`**, **`verification`**, **`blockedBy`**

**Row `status`:** **`pending`**, **`ready`**, **`in_progress`**, **`done`**, **`cancelled`**, **`failed`**

**Rules**

- Treat **`[TASK_BOARD]`** in the injected runtime context as the authoritative **compact** snapshot.
- If **`[TASK_BOARD]`** is empty and the task is multi-step, call **`task_board:init`** in the first round.
- `task_board:patch` should update only the current task id from **`[TASK_BOARD]`**.
- Mark **`done`** only when the current task goal is already achieved in observable evidence.
- If the root tool is a new action to achieve that goal, patch **`in_progress`** this turn (or skip `done`).
- Keep milestones small (roughly **3–12** rows). Use **`local_*`** ids only on **child** boards (sub-agents).
- Keep row text compact; avoid long prose in `title` / `output` / `verification` to reduce prompt tokens.
- **Do not** patch the parent milestone board from a child agent (use **`sync_finding`** or let the host report completion).
- After **`retry_count >= 2`** on a stuck row, diagnose in **`thoughts`** before the next **`patch`**.
- For workers that emit **`verify:report`**, use it for sidecar ordering and evidence context:
  - First initialization round may omit `verify:report`.
  - After init, run `verify:report` first, then `task_board:patch`.
  - Transition into the next milestone only after the current task goal is complete.

**`items` in `tool_args`**

Pass a JSON **array** of row objects, or a JSON **string** containing that array (escaped quotes required).

#### Example (goal already met -> done)

```json
{
  "thoughts": "Goal met: success toast is visible and the new item appears in the list.",
  "headline": "Complete current task",
  "tool_name": "response",
  "tool_args": {
    "text": "Current task completed: success toast shown and new row visible in the target list."
  },
  "sidecar_tools": [
    {
      "tool_name": "verify:report",
      "tool_args": {
        "action_result": "...",
        "repetition_count": "...",
        "failure_cause": "..."
      }
    },
    {
      "tool_name": "task_board:patch",
      "tool_args": {
        "items": "[{\"id\":\"current-task\",\"title\":\"Current task title\",\"status\":\"done\",\"verification\":\"goal_met: success toast shown and new row visible in target list\",\"output\":\"completed: created item and confirmed it in UI\"}]"
      }
    }
  ]
}
```
