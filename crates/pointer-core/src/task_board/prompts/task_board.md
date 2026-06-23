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
  **`work_item_delta` / `work_item_claim`** (when enabled), and **`task_board_finalize`**.
- If `[TASK_BOARD]` looks empty, treat it as a planner/host issue — **still do not init**; patch or report in content.
- Treat injected `[TASK_BOARD]` as source of truth.
- **`## Task`** may include **`work_items_source`** (path + row count) after host/planner init — work_items already seeded; execute, do not re-import.

Coder and other agents: unchanged — you may still init/replace yourself.

**Native flat tools** — call by tool name.
**Do not** pass a `method` field in arguments.

- **`task_board_init`**: meta + `global_milestones`; Type2 also `item_milestones` + `work_items`.
  *(Computer + planner: planner-only — not in your tool list.)*
- **`task_board_replace`**: **only** full `item_milestones[]` (execution SOP refresh).
- **`task_board_patch`**: one row update (see **Patch**).
- **`task_board_prune`**: cancel pending rows (`ids`).
- **`task_board_finalize`**: mark board complete after all rows are terminal.
- **`task_board_sync_finding`**: child board sync to parent findings.
- **`task_board_check_deps`**: inspect dependency readiness for one row.

Tool result is compact.
Treat injected `[TASK_BOARD]` as source of truth.

Row status:
`pending`, `ready`, `in_progress`, `done`, `cancelled`, `failed`.

## Document shape (v4)

| Layer | Field | When |
| --- | --- | --- |
| Meta | `goal`, `context`, `constraint`, `done_when` | always on init |
| Meta | `work_item_mode`, `expected_total`, `dynamic_quota` | Type2 only |
| Task-level | `global_milestones[]` | Type1 full flow; Type2 fixed `g_plan` / `g_exec` / `g_deliver` |
| Per-item SOP | `item_milestones[]` | Type2 only; template with `{field}` placeholders from work_item payload |
| Queue | `work_items` (DB) | Type2; seed on **init** only |

**Type 1** (no work_items): only `global_milestones[]` — 3–12 steps or user steps.

**Type 2** (enumerated / dynamic work_items):
- `global_milestones` = three fixed ids: `g_plan`, `g_exec`, `g_deliver`.
  **`title`** = one-line phase summary (UI row label); not a 2-character fixed label.
- `item_milestones` = reusable SOP template per work_item (no deliver step).
- Delivery lives in **`g_deliver`**, not in item rows.

## Row fields

Each milestone row (`global_milestones` or `item_milestones`) may include:

| Field | Patch | Content |
| --- | --- | --- |
| `plan` | replace | How to execute (markdown). |
| `constraint` | replace | Row-level constraint; may inherit `meta.constraint`. |
| `done_when` | replace | Outcome acceptance criteria (markdown). |
| `remark` | replace | Short outcome note when marking `done` (optional). |
| `delivery_format` | replace | Only on `g_deliver` (`xlsx`, `csv`, …). |

**Removed in v4 (do not send):**
`progress`, `validate_requirement`, `validate_results`, `validate_*_delta`,
`extract_*`, row-level `work_item_mode`.

User-facing delivery belongs in **assistant `content`**, not board row fields.

## `action_verify` vs `done_when`

- **`action_verify`** (sidecar): validates a **single UI step**.
  On **`action_result=pass`**, set **`step_summary`** (one line).
- **`done_when` / `remark`**: validates the **milestone outcome**.
- Injected **`[TASK_BOARD]`** shows `done_when` on the current row;
  completed rows may show `remark` under **Global milestones** / **Item milestones**.

Do not paste `action_verify` JSON into `remark`.

## Patch

Each call updates **one** milestone row.
Never batch multiple rows or multiple work_items in one patch.

| Task type | Parameter | Exactly one row |
| --- | --- | --- |
| Type1 / global step | `global_milestones` | `[{ id, status, … }]` |
| Type2 / item SOP step | `milestones` | `[{ id, status, … }]` |

**Do not** send both `global_milestones` and `milestones` in the same patch.

Pass **JSON objects and arrays** in tool arguments — do **not** stringify
`milestones`, `work_item_delta`, or `work_item_claim` (host tolerates strings but objects are required for reliable parsing).

`work_item_delta.id` is the **auto-assigned integer seq** within the current store (`1`, `2`, …).
Copy it **verbatim** from **`[WORK_ITEM_FOCUS]`** (`id=N` → `"id": N` in JSON).
Do not construct, guess, or reuse ids from earlier turns.

### Type 2 execution cadence (mandatory)

When inject shows **`## Item milestones`** and **`[WORK_ITEM_FOCUS]`** (Type2, `g_exec` active):

**Same turn as evidence** — call **`task_board_patch`**; do not defer board updates while taking more desktop actions.

| Event | Patch |
| --- | --- |
| One **item SOP step** completed (`done_when` met for current `item_milestone`) | **`milestones`**: exactly one row update (e.g. current → `done`, next → `in_progress`) |
| One **work_item** fully completed (all SOP steps for the focused row) | **`milestones`** (final step) **+** **`work_item_delta`** (terminal status + `result_summary`) |

Rules:

- Patch **every** completed SOP step — not only at queue row boundaries.
- Patch **every** terminal work_item — host resets **`item_milestones`** to `pending` for the next row.
- **`work_item_delta.id`**: copy only from **`[WORK_ITEM_FOCUS]`** in **this** turn's inject; never invent ids.
- Do **not** patch **`global_milestones`** when finishing a single work_item (only SOP + delta).

### Type 1 — advance a global step

```json
{
  "global_milestones": [{ "id": "m1", "status": "done", "remark": "root cause in auth/handler.rs" }]
}
```

### Type 2 — advance current item SOP step

```json
{
  "milestones": [{ "id": "m2", "status": "done" }]
}
```

### Type 2 — complete one work_item

Same patch as SOP step, plus **one** `work_item_delta`:

```json
{
  "milestones": [{ "id": "m2", "status": "done" }],
  "work_item_delta": {
    "id": 1,
    "status": "done",
    "result_summary": "item completed; acceptance criteria met"
  }
}
```

Host resets **`item_milestones`** to `pending` after `work_item_delta` terminal.
**Do not** patch `global_milestones` when finishing a work_item.

### Type 2 — global exec / deliver

Only when inject shows `exec_met: true` (or Host auto-completed):

```json
{
  "global_milestones": [{ "id": "g_exec", "status": "done" }]
}
```

`g_deliver` requires `g_exec` done and no `in_progress` work_item:

```json
{
  "global_milestones": [{ "id": "g_deliver", "status": "in_progress" }]
}
```

Then call **`work_items_export`**, attach `MEDIA`, then:

```json
{
  "global_milestones": [{ "id": "g_deliver", "status": "done" }]
}
```

### Dynamic work_items

One `work_item_claim` or `work_item_delta` per patch while `g_exec` is active.

## Replace (execution only)

**`task_board_replace`** replaces the **full** `item_milestones[]` table only.

- Forbidden: `goal`, `global_milestones`, `work_items`, meta fields.
- Submit the **complete** array including current `status` values.
- Host does not merge by id — missing progress is lost if you omit statuses.
- Use when SOP steps need restructuring mid-run, not for scope/goal changes.

## Core rules

- **Computer + planner:** never call `init` during execution — use **`task_board_patch`** / **`task_board_replace`** only.
- **Coder / Computer without planner:** if `[TASK_BOARD]` is empty and work is multi-step, call **`task_board_init`**.
- **Type2:** patch **each** completed item SOP step and **each** terminal work_item in the **same turn** as the evidence (see **Type 2 execution cadence**).
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

Order (host-built):

1. **## Task** — goal, context, constraint, done_when (+ Type2 mode/total)
2. **## Global milestones**
3. **## Current global_milestone**
4. **## Current global_milestone plan** (separate section)
5. **## Item milestones** + **## Current item_milestone** — only Type2 while `g_exec` is `in_progress`
6. **## Work items** — Type2 window + `[WORK_ITEM_FOCUS]`

While `g_exec` is `done` or `g_deliver` is active, Item blocks are **omitted**.
`plan_resolved` / `done_when_resolved` come from `[WORK_ITEM_FOCUS]` substitution.

## Final delivery (user summary)

When writing the **final summary** in assistant **`content`**:

- **Source of truth:** injected **`[TASK_BOARD]`** in this turn.
- Read `remark` on each **`done`** row under **Global milestones**.
- For Type2, read `result_summary` on terminal work_items.
- Call **`finalize`** in the same turn as the final summary when every row is terminal.

## Profile guidance

Computer (with `action_verify`):

- **With host planner:** board already exists — execute with patch/replace; do not init.
- **Without planner:** init when expected operation steps >3, or **>5** similar repetitive operations.
- Type2: enumeration is seeded at planner init; per-item SOP lives in **`item_milestones`**.
- **Type2 cadence (mandatory):** after `action_verify` pass (or equivalent evidence), **`task_board_patch`** same turn for each completed SOP step or terminal work_item.
- Copy **`work_item_delta.id`** only from **`[WORK_ITEM_FOCUS]`** — never construct ids.

Engineering profiles:

- Put acceptance criteria in **`done_when`**.
- Put evidence in **`remark`** when marking a row `done`.

## Init input

**Type 1 example — `task_board_init`**

```json
{
  "goal": "Ship feature X",
  "context": "User needs login fix only",
  "constraint": "crates/auth only",
  "done_when": "tests pass + manual login OK",
  "global_milestones": [
    {
      "id": "m1",
      "title": "Locate code",
      "status": "pending",
      "done_when": "handler file identified"
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
    { "id": "g_plan", "title": "Seed SOP and work_items from source", "status": "done", "done_when": "SOP + all rows seeded" },
    { "id": "g_exec", "title": "Run per-item flow for every work_item", "status": "in_progress", "done_when": "all rows terminal with result_summary" },
    { "id": "g_deliver", "title": "Export results and attach MEDIA", "status": "pending", "delivery_format": "xlsx", "done_when": "export + MEDIA" }
  ],
  "item_milestones": [
    { "id": "m1", "title": "Open target form", "status": "pending", "done_when": "form ready for input" },
    { "id": "m2", "title": "Apply row fields", "status": "pending", "plan": "Fill fields from {title} payload", "done_when": "fields match row data" },
    { "id": "m3", "title": "Save and confirm", "status": "pending", "done_when": "save succeeded" }
  ],
  "work_items": [
    { "title": "Row A", "payload": { "title": "Row A" } }
  ]
}
```

Legacy alias: `items` on init maps to `global_milestones`.

Examples use **`function.name`** + **`function.arguments`** only.
