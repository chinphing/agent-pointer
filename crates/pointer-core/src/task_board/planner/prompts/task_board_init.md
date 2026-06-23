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
- Example: `"/Users/me/data/cities.xlsx"` or `pointer-media://conv-id/att_id_file.xlsx`
- Do not pass `{ "format": "xlsx" }` without `path` / `ref`.

**Optional meta**
- `context`, `constraint`, `done_when` at document level.

**Limits**
- Max 20 global rows (Type1).

**After success**
- Do not call more tools; reply briefly if needed.

---

### Example — Type 2 enumerated (Excel city list)

```json
{
  "goal": "Add each city from the attached list as a work address in BOSS",
  "work_item_mode": "enumerated",
  "expected_total": 127,
  "work_items_source": "/path/to/cities.xlsx",
  "global_milestones": [
    { "id": "g_plan", "title": "Load city list and confirm login", "status": "pending", "done_when": "127 rows seeded; BOSS form open" },
    { "id": "g_exec", "title": "Add each city as work address", "status": "pending", "done_when": "all work_items terminal with result_summary" },
    { "id": "g_deliver", "title": "Verify completion", "status": "pending", "delivery_format": "xlsx", "done_when": "export attached" }
  ],
  "item_milestones": [
    { "id": "m1", "title": "Open address field", "status": "pending", "done_when": "address input focused" },
    { "id": "m2", "title": "Enter city and save", "status": "pending", "plan": "Type {city} → select → save", "done_when": "{city} saved" }
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
