### `task_board`

Session-scoped working memory for multi-step execution.

If work is multi-step, initialize early.
Single-step work may skip the board.

### Computer + host planner (execution only)

When the host runs the task-board planner before your turn:

- **Do not** call `task_board_init` — the planner already built the board.
- Use **`task_board_patch`**, **`task_board_replace`** (item SOP only),
  **`work_item_delta` / `work_item_claim`** (when enabled), and **`task_board_finalize`**.
- Treat injected `[TASK_BOARD]` as source of truth.

Coder and other agents: unchanged — you may still init/replace yourself.

**Native flat tools** — call by tool name.
**Do not** pass a `method` field in arguments.

- **`task_board_init`**: meta + `global_milestones`; Type2 also `item_milestones` + `work_items`.
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
| Per-item SOP | `item_milestones[]` | Type2 only; template with `{city}` placeholders |
| Queue | `work_items` (DB) | Type2; seed on **init** only |

**Type 1** (no work_items): only `global_milestones[]` — 3–12 steps or user steps.

**Type 2** (enumerated / dynamic work_items):
- `global_milestones` = **计划 → 执行 → 交付** (`g_plan`, `g_exec`, `g_deliver`).
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
    "id": "wi_conv_000004",
    "status": "done",
    "result_summary": "深圳: 5 contacts saved"
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

- If `[TASK_BOARD]` is empty and task is multi-step, call `init`.
- Patch every turn that completes one SOP step or one work_item.
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

- Initialize when expected operation steps >3, or **>5** similar repetitive operations.
- Type2: put enumeration in **`work_items`** on init, SOP in **`item_milestones`**.
- Cadence: `action_verify` → **`task_board_patch`** same turn when a step completes.

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
  "goal": "BOSS 10-city contact harvest",
  "work_item_mode": "enumerated",
  "expected_total": 10,
  "global_milestones": [
    { "id": "g_plan", "title": "计划", "status": "done", "done_when": "SOP + 10 wi seeded" },
    { "id": "g_exec", "title": "执行", "status": "in_progress", "done_when": "all wi terminal + result_summary" },
    { "id": "g_deliver", "title": "交付", "status": "pending", "delivery_format": "xlsx", "done_when": "export + MEDIA" }
  ],
  "item_milestones": [
    { "id": "m1", "title": "确认登录", "status": "pending", "done_when": "logged in" },
    { "id": "m2", "title": "搜索保存", "status": "pending", "plan": "切换至 {city} → …", "done_when": "{city} result_summary written" }
  ],
  "work_items": [
    { "title": "北京", "payload": { "city": "北京" } }
  ]
}
```

Legacy alias: `items` on init maps to `global_milestones`.

Examples use **`function.name`** + **`function.arguments`** only.
