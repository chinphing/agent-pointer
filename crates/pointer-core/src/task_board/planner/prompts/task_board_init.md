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
  Set milestone **`done_when`** for acceptance criteria when known.
  Optional: `plan`, `rules`, `constraints`.

**Type 2 additionally required**
- `work_item_mode`: **`enumerated`** or **`dynamic`** (see decision below).
- Fixed globals: `g_plan`, `g_exec`, `g_deliver` (ids fixed; **`title`** = phase summary).
- `item_milestones` — SOP template (placeholders `{field}` from work_item `payload`).

### User rules on init

Copy user-specified normative rules into **milestone `rules`** (not `work_items[]`).
Distill short iron laws into milestone **`constraints`** (text; multiple bullet lines OK).
Use **`plan`** for procedure only; **`done_when`** for step exit checks.
Task-wide binding bullets → **`meta.constraints`**.

### `work_item_mode` decision

**Use `enumerated` when** the full target list is known up front:
- User attached CSV/XLSX/JSONL, or
- You have the complete row list inline in `work_items[]`.

Required with `enumerated`:
- `expected_total` — row count after seed.
- Seed with **one** of:
  - `work_items_source` — string path (see below), **or**
  - `work_items[]` inline.

**Use `dynamic` when** only a quota is fixed and the Agent picks targets at runtime.

Required with `dynamic`:
- `dynamic_quota` — max units for this campaign.
- **Do not** pass `work_items_source` or `work_items[]`.

Host rejects: `dynamic` + file path, or `enumerated` without seed.

**`work_items_source` (enumerated file list only)**
- Pass a **plain string** — same path forms as user attachments / `media_understand`.
- Do not pass `{ "format": "xlsx" }` without `path` / `ref`.

**Optional meta**
- `context`, `constraints`, `done_when` at document level.

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
    { "id": "g_plan", "title": "Load source list and open target UI", "status": "pending", "done_when": "all rows seeded; entry point ready" },
    { "id": "g_exec", "title": "Run per-row workflow for each work_item", "status": "pending", "done_when": "all work_items terminal with result_summary" },
    { "id": "g_deliver", "title": "Verify completion and export", "status": "pending", "delivery_format": "xlsx", "done_when": "export attached" }
  ],
  "item_milestones": [
    {
      "id": "m1",
      "title": "Open row form",
      "status": "pending",
      "rules": "User norms for this step when provided.",
      "constraints": "- Do not proceed without form ready",
      "plan": "Open form → confirm fields visible",
      "done_when": "form ready for row data"
    },
    {
      "id": "m2",
      "title": "Apply row fields and save",
      "status": "pending",
      "plan": "Fill from {title} payload → confirm → save",
      "done_when": "{title} saved successfully"
    }
  ]
}
```

### Example — Type 2 dynamic (runtime target selection)

```json
{
  "goal": "Process up to N matching targets from a runtime queue",
  "work_item_mode": "dynamic",
  "dynamic_quota": 50,
  "global_milestones": [
    { "id": "g_plan", "title": "Open workflow and define selection rules", "status": "pending", "done_when": "entry point ready; criteria clear" },
    { "id": "g_exec", "title": "Claim and process matching targets", "status": "pending", "done_when": "quota reached or no more matches" },
    { "id": "g_deliver", "title": "Summarize batch outcome", "status": "pending", "done_when": "counts reported" }
  ],
  "item_milestones": [
    { "id": "m1", "title": "Claim target", "status": "pending", "done_when": "work_item claimed" },
    { "id": "m2", "title": "Complete unit work", "status": "pending", "done_when": "result_summary set" }
  ]
}
```
