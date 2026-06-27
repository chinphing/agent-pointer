Create a new task board when `[CURRENT_TASK_BOARD]` is empty.

**When to call**
- Multi-step task and board is empty.

**When not to call**
- Board already has milestones (use **`task_board_replace`** for item SOP-only updates).
- Single-step task.

**Required**
- `goal` — one sentence outcome.
- `global_milestones` — task-level rows (`id`, `title`, `status`).
  **`title`**: one-line summary shown in UI (not a fixed 2-character label).

**Type 2 additionally required**
- `work_item_mode`: **`enumerated`** or **`dynamic`** (see decision below).
- Fixed globals: `g_plan`, `g_exec`, `g_deliver` (ids fixed; **`title`** = phase summary).
- `item_milestones` — SOP template (placeholders `{field}` from work_item `payload`).

### Field reference (init)

Copy user-stated norms on init — do not drop them.

**Meta (document level)**

| Field | Put | Do not put |
| --- | --- | --- |
| **`goal`** | One-sentence task outcome | Steps, rules, acceptance detail |
| **`context`** | Background facts (non-normative) | Rules, constraints, acceptance |
| **`constraints`** | Task-wide iron laws (text; multiple bullet lines OK) | Per-step norms; procedure |
| **`done_when`** | Whole-board success criteria | Single-step exit checks |

**Each row in `global_milestones[]` or `item_milestones[]`**

| Field | Put | Do not put |
| --- | --- | --- |
| **`rules`** | User/session normative text for this step/phase | Procedure steps; one-line completion checks |
| **`constraints`** | Must-not / must-always bullets (multi-line text OK) | Full algorithms; `done_when` text |
| **`plan`** | How to execute: tool order, inputs/outputs, placeholders | User rule bodies; acceptance criteria |
| **`done_when`** | Verifiable exit condition before marking `done` | Rule library; operation manual |
| **`delivery_format`** | Export format on `g_deliver` only (`xlsx`, `csv`, …) | — |

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

**Limits**
- Max **12** global rows (Type1).

**Type2 global status after seed**
- When **`enumerated`** seed succeeds (`work_items_source` or `work_items[]`), set
  `g_plan: done`, `g_exec: in_progress`, `g_deliver: pending`.
- If you pass all globals as `pending`, the host still auto-advances `g_plan` / `g_exec`
  after seed — prefer the statuses above so `[CURRENT_TASK_BOARD]` matches runtime.
- **`dynamic`** (no seed on init): keep globals `pending` until execution claims targets.

**After success**
- Do not call more tools on the next round unless the board already existed and only
  **`item_milestones`** (rules / constraints / plan / steps) need a refresh — then
  **`task_board_replace`** once.

---

### Example — Type 1

```json
{
  "goal": "Ship feature X",
  "context": "Background facts only",
  "constraints": "- Scope locked after init\n- Do not guess file paths",
  "done_when": "All milestones terminal with evidence",
  "global_milestones": [
    {
      "id": "m1",
      "title": "Locate code",
      "status": "pending",
      "rules": "User-specified norms for this step (if any).",
      "constraints": "- Read-only recon until m1 done",
      "plan": "Search repo → open candidate files",
      "done_when": "Handler file identified"
    }
  ]
}
```

### Example — Type 2 enumerated (attached row list)

```json
{
  "goal": "Process every row in the attached source list through the target workflow",
  "constraints": "- Do not skip validation on any row",
  "done_when": "All work_items terminal; export attached",
  "work_item_mode": "enumerated",
  "expected_total": 42,
  "work_items_source": "/path/to/batch_list.xlsx",
  "global_milestones": [
    { "id": "g_plan", "title": "Load source list and open target UI", "status": "done", "done_when": "all rows seeded; entry point ready" },
    { "id": "g_exec", "title": "Run per-row workflow for each work_item", "status": "in_progress", "done_when": "all work_items terminal with result_summary" },
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
