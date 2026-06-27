Replace the **item SOP template** (`item_milestones[]` only).

**When to call**
- Board **already exists** and the per-item SOP must be updated before or during planning:
  refresh step list, **`rules`**, **`constraints`**, **`plan`**, or **`done_when`** on template rows.
- Execution Agent may also call this when `g_exec` is active and SOP steps must be restructured mid-run.

**When not to call**
- Board empty (use **`task_board_init`**).
- Need to change **`goal`**, **`work_items`** list, **`global_milestones`**, or meta fields.
- User only said "continue" on an unchanged SOP.

**Required**
- `item_milestones` — **full** new template array with current `status` values.
  Include updated **`rules`** / **`constraints`** when user norms changed.

**Forbidden**
- `goal`, `context`, `global_milestones`, `work_items`, meta fields.

**After success**
- Host replaces the whole `item_milestones` table (no merge by id).
- Stop calling tools on the next round unless another single replace is still required.
