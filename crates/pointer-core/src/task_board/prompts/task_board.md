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

- Treat **`[TASK_BOARD]`** in the system prompt as the authoritative **compact** snapshot.
- If **`[TASK_BOARD]`** is empty and the task is multi-step, call **`task_board:init`** in the first round.
- Do not mark **`done`** without verification evidence in-thread, or an explicit risk note.
- Keep milestones small (roughly **3–12** rows). Use **`local_*`** ids only on **child** boards (sub-agents).
- Keep row text compact; avoid long prose in `title` / `output` / `verification` to reduce prompt tokens.
- **Do not** patch the parent milestone board from a child agent (use **`sync_finding`** or let the host report completion).
- After **`retry_count >= 2`** on a stuck row, diagnose in **`thoughts`** before the next **`patch`**.
- For workers that emit **`verify:report`**, use it as the status source:
  - First initialization round may omit `verify:report`.
  - After init, run `verify:report` first, then `task_board:patch`.
  - Transition into the next milestone only after the report closes the previous one.

**`items` in `tool_args`**

Pass a JSON **array** of row objects, or a JSON **string** containing that array (escaped quotes required).

#### Example (sidecar + terminal)

```json
{
  "thoughts": "Mark tests in progress then run them.",
  "headline": "Verify",
  "sidecar_tools": [
    {
      "tool_name": "task_board:patch",
      "tool_args": {
        "items": "[{\"id\":\"run-tests\",\"title\":\"Run tests\",\"status\":\"in_progress\",\"verification\":\"cargo test -p pointer-core\"}]"
      }
    }
  ],
  "tool_name": "terminal",
  "tool_args": {
    "command": "cargo test -p pointer-core"
  }
}
```
