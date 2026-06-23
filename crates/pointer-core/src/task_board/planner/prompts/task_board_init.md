Create a new task board when `[CURRENT_TASK_BOARD]` is empty.

**When to call**
- Multi-step task and board is empty.

**When not to call**
- Board already has milestones.
- Single-step task.

**Required**
- `goal` — one sentence outcome.
- `global_milestones` — task-level rows (`id`, `title`, `status: pending`).
  **`title`**: one-line summary shown in UI (not a fixed 2-character label).
  Also set `done_when` for acceptance criteria.
  Optional: `plan`, `constraint`.

**Type 2 additionally required**
- `work_item_mode`: **`enumerated`** or **`dynamic`** (see decision below).
- Fixed globals: `g_plan`, `g_exec`, `g_deliver` (ids fixed; **`title`** = phase summary).
- `item_milestones` — SOP template (placeholders `{field}` from work_item `payload`).

### `work_item_mode` decision

**Use `enumerated` when** the full target list is known up front:
- User attached CSV/XLSX/JSONL, or
- You have the complete row list inline in `work_items[]`.

Required with `enumerated`:
- `expected_total` — row count after seed.
- Seed with **one** of:
  - `work_items_source` — string path (see below), **or**
  - `work_items[]` inline.

**Use `dynamic` when** only a quota is fixed and the Agent picks targets at runtime
(e.g. "greet the next 50 matching candidates").

Required with `dynamic`:
- `dynamic_quota` — max units for this campaign.
- **Do not** pass `work_items_source` or `work_items[]`.

Host rejects: `dynamic` + file path, or `enumerated` without seed.

**`work_items_source` (enumerated file list only)**
- Pass a **plain string** — same path forms as user attachments / `media_understand`:
  - **`localPath`** absolute path from Lead context
  - **`pointer-media://…`** media ref from attachment manifest
  - Storage-relative path under conversation media
- Example: `"/Users/me/data/batch_list.xlsx"` or `pointer-media://conv-id/att_id_file.xlsx`
- Do not pass `{ "format": "xlsx" }` without `path` / `ref`.

**Optional meta**
- `context`, `constraint`, `done_when` at document level.

**Limits**
- Max 20 global rows (Type1).

**After success**
- Do not call more tools; reply briefly if needed.

---

### Example — Type 2 enumerated (attached row list)

```json
{
  "goal": "Process every row in the attached source list through the target workflow",
  "work_item_mode": "enumerated",
  "expected_total": 42,
  "work_items_source": "/path/to/batch_list.xlsx",
  "global_milestones": [
    { "id": "g_plan", "title": "Load source list and open target UI", "status": "pending", "done_when": "all rows seeded; workflow entry point ready" },
    { "id": "g_exec", "title": "Run per-row workflow for each work_item", "status": "pending", "done_when": "all work_items terminal with result_summary" },
    { "id": "g_deliver", "title": "Verify completion and export", "status": "pending", "delivery_format": "xlsx", "done_when": "export attached" }
  ],
  "item_milestones": [
    { "id": "m1", "title": "Open row form", "status": "pending", "done_when": "form ready for row data" },
    { "id": "m2", "title": "Apply row fields and save", "status": "pending", "plan": "Fill from {title} payload → confirm → save", "done_when": "{title} saved successfully" }
  ]
}
```

### Example — Type 2 dynamic (runtime target selection)

```json
{
  "goal": "Greet up to 50 qualified candidates in the recruitment inbox",
  "work_item_mode": "dynamic",
  "dynamic_quota": 50,
  "global_milestones": [
    { "id": "g_plan", "title": "Open inbox and define filter rules", "status": "pending", "done_when": "inbox visible; criteria clear" },
    { "id": "g_exec", "title": "Claim and greet matching candidates", "status": "pending", "done_when": "50 done or no more matches" },
    { "id": "g_deliver", "title": "Summarize outreach", "status": "pending", "done_when": "counts reported" }
  ],
  "item_milestones": [
    { "id": "m1", "title": "Claim target", "status": "pending", "done_when": "work_item claimed" },
    { "id": "m2", "title": "Send greeting", "status": "pending", "done_when": "message sent; result_summary set" }
  ]
}
```
