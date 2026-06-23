Create a new task board when `[CURRENT_TASK_BOARD]` is empty.

**When to call**
- Multi-step task and board is empty.

**When not to call**
- Board already has milestones.
- Single-step task.

**Required**
- `goal` — one sentence outcome.
- `global_milestones` — task-level rows (`id`, `title`, `status: pending`).
  Recommended per row: `plan`, `constraint`, `done_when`.

**Type 2 additionally required**
- `work_item_mode`: `enumerated` or `dynamic`.
- `expected_total` (enumerated) or `dynamic_quota` (dynamic).
- Fixed globals: `g_plan`, `g_exec`, `g_deliver`.
- `item_milestones` — SOP template (placeholders `{field}` from work_item `payload`).
- `work_items[]` inline (≤50) or `work_items_source` path.

**Optional meta**
- `context`, `constraint`, `done_when` at document level.

**Limits**
- Max 20 global rows (Type1).
- Max 50 inline `work_items` on init.

**After success**
- Do not call more tools; reply briefly if needed.
