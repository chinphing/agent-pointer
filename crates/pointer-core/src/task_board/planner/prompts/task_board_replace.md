Replace the **item SOP template** during execution.

**When to call**
- Board exists, `g_exec` is active, and SOP steps must be restructured.
- Execution Agent only — Planner does **not** call this.

**When not to call**
- Board empty (use `task_board_init`).
- Need to change `goal`, `work_items` list, or `global_milestones`.
- User only said "continue" on same SOP.

**Required**
- `item_milestones` — **full** new template array with current `status` values.

**Forbidden**
- `goal`, `context`, `global_milestones`, `work_items`, meta fields.

**After success**
- Host replaces the whole `item_milestones` table (no merge by id).
