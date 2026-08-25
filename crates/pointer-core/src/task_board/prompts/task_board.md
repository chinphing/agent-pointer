### `task_board`

Session-scoped working memory for multi-step execution.

Treat injected **`[TASK_BOARD]`** as source of truth.

**Native flat tools** — call by tool name.
**Do not** pass a `method` field in arguments.

- **`task_board_init`**: meta + `global_milestones[]` — see **Board shapes** and **Init examples**.
  `global_milestones` is a JSON array of objects (`id`, `title`, `status`).
  Do not quote the array as a string. At least one row is required.
- **`task_board_patch`**: one milestone row via **`milestones[]`** — see **Patch**.
- **`task_board_prune`**: cancel pending rows (`ids`).
- **`task_board_finalize`**: mark board complete after all rows are terminal.
- **`task_board_abandon`**: mark a **running** board **failed** (scope ended early; host stops injecting it).
- **`task_board_check_deps`**: inspect dependency readiness for one row.

Tool result is compact.

Row status:
`pending`, `ready`, `in_progress`, `done`, `cancelled`, `failed`.

## Board shapes

The stored table is **`global_milestones[]`**.
Patch it with **`task_board_patch`** **`milestones[]`** (one row).
Do not send **`global_milestones`** on patch.

### Linear (non-loop)

3–8 milestone rows (`m1`, `m2`, …). Each row may carry `plan`, `rules`, `constraints`, `done_when`.

Host inject shows the full step ladder under **All tasks**; patch one row per verified step.

### Loop / batch

`g_plan` → **`wi_*` item rows** → `g_deliver`:

| Row | Role |
| --- | --- |
| `g_plan` | Setup + **shared per-item procedure** in `plan` when steps repeat across all `wi_*` |
| `wi_1` … `wi_n` | One row per target — **title** (+ optional per-target `plan` / `done_when` overrides) |
| `g_deliver` | Final delivery (`delivery_format` when exporting a file) |

**Loop rules**

- Put the **shared per-item procedure** in **`g_plan.plan`** once; do not repeat on every `wi_*` unless one target differs.
- Patch **one item row** to `done` / `failed` when that target is complete — not per GUI micro-step.
- Host auto-advances the next `wi_*` to `in_progress`.
- Max **100** rows in **`global_milestones`** (loop: up to **~98** **`wi_*`** plus **`g_plan`** and **`g_deliver`**).

### Loop seeding: enumerated vs dynamic

Two ways to create **`wi_*`** rows between **`g_plan`** and **`g_deliver`**.
Choose from **what you know at init time** — not from convenience.

| | **Enumerated (static list)** | **Dynamic (runtime quota)** |
| --- | --- | --- |
| **Use when** | Every target identity is already known | Only **how many** is known; identities appear later |
| **Init fields** | **`g_plan`** + every **`wi_*`** + **`g_deliver`** in **`global_milestones`** | **`g_plan`** + **`g_deliver`** in **`global_milestones`**, plus **`dynamic_quota`** (host seeds **`wi_*`**) |
| **Row titles at init** | Real target id in each **`wi_*` `title`** | Host placeholders **`#1`…`#N`** |
| **Before exec** | Titles set — follow **`g_plan.plan`** | Patch active row **`title`** when target id is known |
| **Shared procedure** | **`g_plan.plan`** (+ optional **`g_plan.done_when`** for item exit) | Same — write on **`g_plan`** at init, not on each **`wi_*`** |

**Prefer enumerated when**

- User, goal, or **Assigned task** lists numbered targets (phones, names, ids, …).
- **Context** spells out the full set — add one **`wi_*`** row per target in **`global_milestones`** now.
- Each target needs its own row **`title`**. Shared GUI steps belong in **`g_plan.plan`**, not copied to every row.

**Prefer dynamic when**

- User gives a **quota only** ("process 10 samples") without naming them.
- Targets will be **discovered during the run** (next lead on screen, scroll queue, …).
- Identities are genuinely unknown at init — host seeds **`wi_1`…`wi_N`** with **`#N`** titles via **`dynamic_quota`**.

**Inject / UI during loop exec**

- Host shows **`wi_*` item rows only** (not **`g_plan`** / **`g_deliver`**).
- Board total = **`g_plan`** + N items + **`g_deliver`**; progress **`k/N`** counts **items**, not bookends.
- Mark **`g_plan`** **`done`** on init when setup is already described there or complete.

## Field reference

On init, copy user-stated norms into the matching meta or milestone field — do not drop them.
Milestone **`rules`** on the board override external Skill defaults for this session.

### Meta fields

| Field | Put | Do not put |
| --- | --- | --- |
| **`goal`** | One-sentence task outcome | Steps, rules, acceptance detail |
| **`context`** | Background facts (non-normative) | Rules, constraints, acceptance |
| **`constraints`** | Task-wide iron laws (text; multiple bullet lines OK) | Per-step norms; procedure |
| **`done_when`** | Whole-board success criteria | Single-step exit checks |

### Milestone row fields

Each row in `global_milestones[]`:

| Field | Patch | Put | Do not put |
| --- | --- | --- | --- |
| **`rules`** | replace | User/session normative text for this step/phase; branch logic | Procedure steps; one-line completion checks |
| **`constraints`** | replace | Must-not / must-always bullets | Full algorithms (use `rules`); `done_when` text |
| **`plan`** | replace | How to execute: tool order, inputs/outputs (loop items: full multi-step procedure) | User rule bodies; acceptance criteria |
| **`done_when`** | replace | Verifiable exit before marking `done` | Rule library; operation manual |
| **`remark`** | replace | Short outcome note when marking `done` (optional) | — |
| **`delivery_format`** | replace | Export format on `g_deliver` only (`xlsx`, `csv`, …) | — |

User-facing delivery belongs in **assistant `content`**, not board row fields.

## Host verify vs `done_when`

- **Host verify** (after each desktop tool): validates a **single UI step**; read **`verify:`** suffix on **`[Recent desktop tool calls]`** rows.
- **`done_when` / `remark`**: validates the **milestone outcome**.
- Injected **`[TASK_BOARD]`** shows `done_when` on the current row;
  completed rows may show `remark` under **All tasks (with status)**.

Do not paste verify JSON into `remark`.

## Patch

**Tool:** **`task_board_patch`** only — **one row per call** via **`milestones[]`**.

```json
{ "milestones": [{ "id": "…", "status": "…", "remark": "…" }] }
```

Pass **JSON objects and arrays** — do **not** stringify `milestones`.

### Patch discipline (do not skip)

Injected **`[TASK_BOARD]`** is the session record. Completing work on screen **without** a matching patch loses progress; loop items will not advance.

**Hard rules**

- Patch in the **same assistant message** that closes the row, or the **immediately next** message when verify for that completion lands on a separate turn.
  Never defer across the **next** loop item or linear milestone.
- **`task_board_patch` may share a message with a desktop tool**, or run on a **patch-only turn** (no desktop action that round).
- **One object** in **`milestones[]`** per call — do not batch multiple rows.
- When marking **`done`** or **`failed`**, set **`remark`** (short outcome, e.g. target id + result).
- Read **## Current task** / **in_progress_id** in inject — patch the row that actually moved, not a stale id.

**When to patch**

| Event | Patch |
| --- | --- |
| Loop item **`plan`** fully verified | `wi_*` → `done` + `remark` |
| Loop item terminal failure / skip | `wi_*` → `failed` + `remark` |
| Dynamic target known before exec | optional `wi_*` → set `title`, keep `in_progress` |
| Linear milestone **`done_when`** met | `mN` → `done` + `remark` |
| All loop items terminal | `g_deliver` → `in_progress`, then `done` after delivery |
| Every row terminal + user summary | **`task_board_finalize`** same turn as final **`content`** |

**Loop — not every GUI step.** One **`wi_*`** row = full **`plan`** under **Current task plan**.
Do **not** patch on each micro-step; patch once when that item is terminal.

Host auto-advances the next **`wi_*`** to **`in_progress`** after a terminal item patch.

| Inject shows | Example `id` |
| --- | --- |
| Linear step ladder | `m1`, `m2`, … |
| Loop exec (item rows) | `wi_1`, `wi_2`, … |
| Deliver phase | `g_deliver` |

### Loop exec

When inject shows loop items under **All tasks** and **Current task plan**:

1. Execute **Current task plan** on screen (GUI micro-steps — **no patch yet**).
2. When the **whole item** is verified, **`task_board_patch`** in that message or the next:

```json
{ "milestones": [{ "id": "wi_3", "status": "done", "remark": "13812345678 - 用户不存在" }] }
```

3. Dynamic placeholder — optional before starting the item:

```json
{ "milestones": [{ "id": "wi_3", "title": "13812345678", "status": "in_progress" }] }
```

### Deliver phase

When all loop items are terminal, inject shows **`g_deliver`** only:

```json
{ "milestones": [{ "id": "g_deliver", "status": "in_progress" }] }
```

After user-visible delivery (summary in **`content`**, attach file if `delivery_format` set):

```json
{ "milestones": [{ "id": "g_deliver", "status": "done", "remark": "10/10 processed" }] }
```

### Linear example

```json
{ "milestones": [{ "id": "m1", "status": "done", "remark": "handler located" }] }
```

### Patch tool result (loop progress)

Loop boards return compact progress in the tool result (field **`loop_progress`**):

```json
{
  "loop_progress": {
    "progress": "2/10",
    "done": 2,
    "failed": 0,
    "total": 10,
    "in_progress": 1,
    "pending": 7,
    "in_progress_id": "wi_3"
  }
}
```

Confirm **`in_progress_id`** matches **Current task** before patching.

## Core rules

- During execution, **`task_board_patch` every row transition** — do not rely on inject alone; finalize or prune when appropriate.
- **Linear:** 3–8 milestones; **loop:** only when batch signals (N≥5 enumerated targets or goal asks 汇总/逐条/批量/每个).
- Revise the **same** scope (plan / milestones): **`task_board_replace`** or **`task_board_patch`** — do **not** call **`task_board_init`** again.
- New multi-step scope (or user switched tasks): **`task_board_init`**. Prefer **`task_board_abandon`** then **`task_board_init`**; if you re-init while a board is still running, the host auto-abandons the previous board so only one active parent board remains.
- Cancel obsolete rows with **`task_board_prune`**.
- Finalize in the same turn as final user delivery.

## Parent / child scope

- **Sub-agent (child):** **`[TASK_BOARD]`** = local `global_milestones` only.
  **`[TASK_BOARD_PARENT]`** is read-only — **do not** patch parent rows.
  Report breakthroughs in your final answer to the lead, not on the parent board.
- **Lead (parent):** milestone scope only — no micromanaging child `local_*` rows.

## Injected `[TASK_BOARD]` blocks

1. **## Task** — goal, meta
2. **## All tasks (with status)** — window around current task (3 before + 5 after); omitted rows collapse to one summary line
3. **## Current task** (+ **## Current task plan** / **## Current task rules** when present)

## Final delivery (user summary)

When writing the **final summary** in assistant **`content`**:

- Read `remark` on **`done`** rows under **All tasks (with status)**.
- Call **`task_board_finalize`** in the same turn as the final summary when every row is terminal.

## Init examples

**Linear — `task_board_init`**

```json
{
  "goal": "Ship feature X",
  "context": "Background facts only",
  "constraints": "- Scope locked after init",
  "done_when": "All milestones terminal with evidence",
  "global_milestones": [
    {
      "id": "m1",
      "title": "Locate code",
      "status": "pending",
      "plan": "Search repo → open candidate files",
      "done_when": "Handler file identified"
    },
    {
      "id": "m2",
      "title": "Implement change",
      "status": "pending",
      "plan": "Edit → test",
      "done_when": "Tests pass"
    }
  ]
}
```

**Loop (enumerated) — `task_board_init`**

Targets already in user context — one **`wi_*`** milestone row per target in **`global_milestones`**:

```json
{
  "goal": "Search each phone in WeChat add-contact and record whether an account exists",
  "global_milestones": [
    { "id": "g_plan", "title": "Open WeChat add-contact", "status": "done", "done_when": "Search box ready",
      "plan": "1. Enter phone in search box\n2. Press Enter\n3. Observe result — nickname/avatar = exists; not found = absent\n4. Note outcome" },
    {
      "id": "wi_1",
      "title": "13043841179",
      "status": "pending",
      "done_when": "Result observed and recorded"
    },
    {
      "id": "wi_2",
      "title": "13121928007",
      "status": "pending",
      "done_when": "Result observed and recorded"
    },
    { "id": "g_deliver", "title": "Summarize all results", "status": "pending", "done_when": "User summary sent" }
  ]
}
```

Repeat **`wi_*`** rows for every known target (≤ ~98). Put shared steps on **`g_plan.plan`** once.

**Loop (dynamic quota) — `task_board_init`**

Count known, identities not — only **`g_plan`** / **`g_deliver`** in **`global_milestones`**; host inserts **`wi_1`…`wi_N`** from **`dynamic_quota`**:

```json
{
  "goal": "Process 10 CRM account IDs discovered during the session",
  "dynamic_quota": 10,
  "global_milestones": [
    {
      "id": "g_plan",
      "title": "Prepare",
      "status": "done",
      "plan": "1. Open CRM\n2. Search account ID\n3. Update status field\n4. Save and return to list",
      "done_when": "Status saved or already correct"
    },
    { "id": "g_deliver", "title": "Summarize", "status": "pending", "delivery_format": "xlsx" }
  ]
}
```

Pass these objects as tool arguments (native tool name, no `method` field).
