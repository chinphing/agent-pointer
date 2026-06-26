### `task_board`

Session-scoped working memory for multi-step execution.

**Who creates the board**

| Path | Who calls `task_board_init` |
| --- | --- |
| **Computer + host planner** (default) | Host planner before your first turn — **not you** |
| **Computer without planner** / **Coder** | You, when multi-step and board is empty |

Single-step work may skip the board on any path.

### Computer + host planner (execution only)

When the host runs the task-board planner before your turn:

- **Do not** call `task_board_init` — the planner already built the board; the tool is not available to you.
- Use **`task_board_patch`**, **`task_board_replace`** (item SOP only),
  **`work_item_claim`** (dynamic queue only, when enabled), and **`task_board_finalize`**.
- If `[TASK_BOARD]` looks empty, treat it as a planner/host issue — **still do not init**; patch or report in content.
- Treat injected `[TASK_BOARD]` as source of truth.
- **`## Task`** may include **`work_items_source`** (path + row count) after host/planner init — work_items already seeded; execute, do not re-import.

Coder and other agents: unchanged — you may still init/replace yourself.

**Native flat tools** — call by tool name.
**Do not** pass a `method` field in arguments.

- **`task_board_init`**: meta + `global_milestones`; Type2 also `item_milestones` + `work_items`.
  *(Computer + planner: planner-only — not in your tool list.)*
- **`task_board_replace`**: **only** full `item_milestones[]` (execution SOP refresh).
- **`task_board_patch`**: one row update via **`milestones`** — host routes to the visible inject ladder; see **Patch**.
- **`task_board_prune`**: cancel pending rows (`ids`).
- **`task_board_finalize`**: mark board complete after all rows are terminal.
- **`task_board_sync_finding`**: child board sync to parent findings.
- **`task_board_check_deps`**: inspect dependency readiness for one row.

Tool result is compact.

Row status:
`pending`, `ready`, `in_progress`, `done`, `cancelled`, `failed`.

## Document shape (v4)

| Layer | Field | When |
| --- | --- | --- |
| Meta | `goal`, `context`, `constraints`, `done_when` | always on init |
| Meta | `work_item_mode`, `expected_total`, `dynamic_quota` | Type2 only |
| Task-level | `global_milestones[]` | Type1 full flow; Type2 fixed `g_plan` / `g_exec` / `g_deliver` |
| Per-item SOP | `item_milestones[]` | Type2 only; template with `{field}` placeholders from work_item payload |
| Queue | `work_items` (DB) | Type2; seed on **init** only — `title` + `payload` only |

**Type 1** (no work_items): only `global_milestones[]` — 3–12 steps or user steps.

**Type 2** (enumerated / dynamic work_items):
- `global_milestones` = three fixed ids: `g_plan`, `g_exec`, `g_deliver`.
  **`title`** = one-line phase summary (UI row label); not a 2-character fixed label.
- `item_milestones` = reusable SOP template per work_item (no deliver step).
- Delivery lives in **`g_deliver`**, not in item rows.

## Field reference

On init, copy user-stated norms into the matching meta or milestone field — do not drop them.
Milestone **`rules`** on the board override external Skill defaults for this session.

### Meta fields

Document-level (set on init):

| Field | Put | Do not put |
| --- | --- | --- |
| **`goal`** | One-sentence task outcome | Steps, rules, acceptance detail |
| **`context`** | Background facts (non-normative) | Rules, constraints, acceptance |
| **`constraints`** | Task-wide iron laws (text; multiple bullet lines OK) | Per-step norms; procedure |
| **`done_when`** | Whole-board success criteria | Single-step exit checks |

### Milestone row fields

Each row in `global_milestones[]` or `item_milestones[]`:

| Field | Patch | Put | Do not put |
| --- | --- | --- | --- |
| **`rules`** | replace | User/session normative text for this step/phase; branch logic; field relations | Procedure steps; one-line completion checks |
| **`constraints`** | replace | Must-not / must-always bullets; non-negotiable invariants (multi-line text OK) | Full algorithms (use `rules`); `done_when` text |
| **`plan`** | replace | How to execute this step: tool order, inputs/outputs, placeholders | User rule bodies; acceptance criteria |
| **`done_when`** | replace | Verifiable exit condition before marking `done` | Rule library; operation manual |
| **`remark`** | replace | Short outcome note when marking `done` (optional) | — |
| **`delivery_format`** | replace | Export format on `g_deliver` only (`xlsx`, `csv`, …) | — |

**Removed in v4 (do not send):**
`progress`, `validate_requirement`, `validate_results`, `validate_*_delta`,
`extract_*`, row-level `work_item_mode`.

User-facing delivery belongs in **assistant `content`**, not board row fields.

## `action_verify` vs `done_when`

- **`action_verify`** (sidecar): validates a **single UI step**.
  On **`action_result=pass`**, set **`step_summary`** (one line).
- **`done_when` / `remark`**: validates the **milestone outcome**.
- Injected **`[TASK_BOARD]`** shows `done_when` on the current row;
  completed rows may show `remark` under **All tasks (with status)**.

Do not paste `action_verify` JSON into `remark`.

## Patch

Each call updates **one** row shown under **## All tasks** in inject.

**Always use `milestones`** — one object in the array:

```json
{ "milestones": [{ "id": "…", "status": "…", "remark": "…" }] }
```

Host routes the row to the correct document slice (same projection as inject).
You do **not** choose `global_milestones` vs `item_milestones` at patch time — only at **init**.

| Inject shows | Example `id` | `current_item` |
| --- | --- | --- |
| Step ladder (Type 1) | `m1`, `m2`, … | omit |
| Item SOP template (Type 2 exec) | `m1`, `m2`, … | **`id` required** — copy from **`[WORK_ITEM_FOCUS]`** |
| Deliver row only (Type 2) | `g_deliver` | omit |

**Do not** send `work_item_delta` — host advances the work queue when the **last** template row is `done` / `failed`, **or** when you close the work_item directly (below).

When **`[WORK_ITEM_FOCUS]`** is present, include **`current_item.id`** matching that focus row.

### `current_item` (queue exec)

Optional on every queue patch. Wraps the active work_item row:

| Field | Required | Purpose |
| --- | --- | --- |
| `id` | when `[WORK_ITEM_FOCUS]` present | Must match focus id |
| `status` | Route 2 only | `done` or `failed` — direct close without last milestone |
| `result_summary` | optional | Outcome note when `status: done` |
| `error_message` | optional | Failure note when `status: failed` |

### Two ways to advance the work_item queue

**Route 1 — milestone (preferred when SOP steps were followed):**

```json
{
  "current_item": { "id": "1" },
  "milestones": [{ "id": "m3", "status": "done", "remark": "saved" }]
}
```

Mark the **last** item SOP row `done` or `failed`. Host closes the work_item and starts the next row.

**Route 2 — direct work_item status (when last SOP step was not patched):**

```json
{
  "current_item": {
    "id": "1",
    "status": "done",
    "result_summary": "北京地址已保存"
  }
}
```

Use **`status`: `done`** or **`failed`**. Host applies the same queue advance as Route 1 without requiring the last milestone row to be `done`.

Pass **JSON objects and arrays** — do **not** stringify `milestones` or `work_item_claim`.

### Queue execution cadence (Type 2)

When inject shows item SOP rows (`m1`, `m2`, …) — not `g_deliver`:

**Same turn as evidence** — patch one row; host advances the pointer.

| Event | Your patch |
| --- | --- |
| SOP step completed | `{ "current_item": { "id": "1" }, "milestones": [{ "id": "m1", "status": "done", "remark": "…" }] }` |
| Last SOP step succeeded | `done` + `remark` on **last** template row; host closes work_item and starts next |
| Last SOP step failed | `{ "current_item": { "id": "1" }, "milestones": [{ "id": "m3", "status": "failed", "remark": "…" }] }` |

Do **not** patch next row to `in_progress` — host advances after `done`.
Do **not** patch `g_plan` / `g_exec` during queue execution.

### Deliver phase (`exec_met: true`)

```json
{ "milestones": [{ "id": "g_deliver", "status": "in_progress" }] }
```

Then **`work_items_export`**, attach `MEDIA`, then:

```json
{ "milestones": [{ "id": "g_deliver", "status": "done" }] }
```

### Step mode (Type 1)

When **`[WORK_ITEM_FOCUS]`** is absent:

```json
{ "milestones": [{ "id": "m1", "status": "done", "remark": "handler located" }] }
```

### Dynamic work_items

Use **`work_item_claim`** once per new runtime target (dynamic mode only).
Complete or fail the target via **`milestones`** on the last template row.

## Replace (execution only)

**`task_board_replace`** replaces the **full** `item_milestones[]` table only.

- Forbidden: `goal`, `global_milestones`, `work_items`, meta fields.
- Submit the **complete** array including current `status` values.
- Host does not merge by id — missing progress is lost if you omit statuses.
- Use when SOP steps need restructuring mid-run, not for scope/goal changes.

## Core rules

- **Computer + planner:** never call `init` during execution — patch/replace only (see above).
- **Coder / Computer without planner:** if `[TASK_BOARD]` is empty and work is multi-step, call **`task_board_init`**.
- **Queue mode:** patch **each** completed SOP step in the **same turn** as evidence (`milestones`).
- Keep 3–12 global milestones for Type1; Type2 uses fixed three globals.
- Cancel obsolete rows with **`task_board_prune`**.
- Finalize in the same turn as final user delivery.
- Loop safety is enforced by **tool rounds** per user message.

## Parent / child scope

- **Sub-agent (child):** **`[TASK_BOARD]`** = local `global_milestones` only.
  **`[TASK_BOARD_PARENT]`** is read-only.
  Use **`task_board_sync_finding`** for breakthroughs; **do not** patch parent rows.
- **Lead (parent):** milestone scope only — no micromanaging child `local_*` rows.

## Injected `[TASK_BOARD]` blocks

1. **## Task** — goal, meta, work_item hints
2. **## All tasks (with status)** — one ladder only (global, item template, or `g_deliver`)
3. **## Current task** (+ **## Current task plan** / **## Current task rules** when present)
4. **## Work items** — compact window + **`[WORK_ITEM_FOCUS]`** (queue mode)

`plan_resolved` / `done_when_resolved` on **## Current task** when placeholders apply.

## Final delivery (user summary)

When writing the **final summary** in assistant **`content`**:

- Read `remark` on **`done`** rows under **All tasks (with status)**.
- For queue mode, read terminal summaries on **## Work items** rows.
- Call **`finalize`** in the same turn as the final summary when every row is terminal.

## Profile guidance

Computer (with `action_verify`):

- **Without planner:** init when expected operation steps >3, or **>5** similar repetitive operations.
- Type2: per-item SOP in **`item_milestones`**; patch **`milestones`** each step — host moves the queue.
- **Queue cadence:** after evidence, **`task_board_patch`** with `milestones` (`done` or last-row `failed` + `remark`).

## Init examples

Map user input to **Field reference** fields on init — do not drop stated rules, constraints, or acceptance criteria.

**Type 1 example — `task_board_init`**

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

**Type 2 example — `task_board_init`**

```json
{
  "goal": "Process every row in the attached source list",
  "work_item_mode": "enumerated",
  "expected_total": 10,
  "global_milestones": [
    { "id": "g_plan", "title": "Seed SOP and work_items", "status": "done", "done_when": "SOP + rows seeded" },
    { "id": "g_exec", "title": "Run per-item flow", "status": "in_progress", "done_when": "all work_items terminal" },
    { "id": "g_deliver", "title": "Export results", "status": "pending", "delivery_format": "xlsx", "done_when": "export attached" }
  ],
  "item_milestones": [
    {
      "id": "m1",
      "title": "Open target form",
      "status": "pending",
      "rules": "User norms for this SOP step (session binding).",
      "constraints": "- Do not skip validation",
      "plan": "Navigate → wait for form ready",
      "done_when": "Form ready for row data"
    },
    {
      "id": "m2",
      "title": "Apply row fields",
      "status": "pending",
      "plan": "Fill from {title} payload → save",
      "done_when": "Row saved successfully"
    }
  ],
  "work_items": [
    { "title": "Row A", "payload": { "key": "A" } }
  ]
}
```

During execution: patch with **`milestones`** (+ **`current_item`** when `[WORK_ITEM_FOCUS]` is present).

Legacy alias: `items` on init maps to `global_milestones`.

Examples use **`function.name`** + **`function.arguments`** only.
