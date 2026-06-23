Create a new task board when `[CURRENT_TASK_BOARD]` is empty.

**When to call**
- Multi-step task and board is empty.

**When not to call**
- Board already has milestones (use `task_board_replace`).
- Single-step task.

**Required**
- `goal` — one sentence outcome.
- `items` — milestone rows (`id`, `title`, `status: pending`).
  Optional per row: `work_items[]`, `work_item_mode`, `depends_on`.

**Limits**
- Max 20 rows.
- Max 50 inline `work_items` per row.

**After success**
- Do not call more tools; reply briefly if needed.
