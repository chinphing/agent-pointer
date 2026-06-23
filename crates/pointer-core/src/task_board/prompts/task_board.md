### `task_board`

Session-scoped working memory for multi-step execution.

If work is multi-step, initialize early.
Single-step work may skip the board.

### Computer + host planner (execution only)

When the host runs the task-board planner before your turn:

- **Do not** call `task_board_init` or `task_board_replace` — the planner already built the board.
- Use **`task_board_patch`**, **`work_item_delta` / `work_item_claim`** (when enabled), and **`task_board_finalize`**.
- Treat injected `[TASK_BOARD]` as source of truth.

Coder and other agents: unchanged — you may still init/replace yourself.

**Native flat tools** — call by tool name. **Do not** pass a `method` field in arguments; the host maps the tool name to the operation.

- **`task_board_init`**: set `goal`, optional `global_context`, and `items`.
- **`task_board_replace`**: replace full board.
- **`task_board_patch`**: merge one current row (see **Patch**).
- **`task_board_prune`**: cancel pending rows (`ids`).
- **`task_board_finalize`**: mark board complete after all rows are terminal.
- **`task_board_sync_finding`**: child board sync to parent findings.
- **`task_board_check_deps`**: inspect dependency readiness for one row.

Tool result is compact.
Treat injected `[TASK_BOARD]` as source of truth.

Row status:
`pending`, `ready`, `in_progress`, `done`, `cancelled`, `failed`.

## Row fields (v3)

Each row may include:

| Field | Patch | Content |
| --- | --- | --- |
| `plan` | replace | How to execute (markdown). |
| `progress` | replace | Position within the milestone (`3/10`, `batch 2/4`, `next=…`). Update on every substantive step. |
| `validate_requirement` | replace | Milestone **outcome** acceptance criteria (markdown). |
| `validate_result_delta` | **append** | One new outcome line per patch while `in_progress`. |
| `extract_requirement` | replace | What to extract (fields/scope, markdown). Optional. |
| `extract_result_delta` | **append** | One new extract line per patch while `in_progress`. Optional. |

User-facing delivery belongs in **assistant `content`**, not board row fields.

## `validate_*` vs `action_verify`

- **`action_verify`** (sidecar): validates a **single step** action (UI/click/type). On **`action_result=pass`**, set **`step_summary`** (one line toward the **user task** / board goal); omit on fail/pending/n/a. Tool result enters chat history for rollup without a board.
- **`validate_requirement` / `validate_result_delta`**: validates the **milestone outcome** while working.
- Injected **`[TASK_BOARD]`** shows recent **`validate_result_delta`** on the current row; completed milestones show full outcome evidence under **All tasks**.

Do not paste `action_verify` JSON into `validate_result_delta`.
Summarize milestone outcome in one short **`validate_result_delta`** line (aligned with **`step_summary`** when both are used).

## Patch

Each call: **one row** in **`items`** (the **current** milestone from **`[TASK_BOARD]`**) + **one atomic item** outcome. Never batch multiple items, multiple milestones, or bulk `done`.

| Mode | Row has | Per patch (while working) | Item list |
| --- | --- | --- | --- |
| **A** | `work_item_mode` | `status: in_progress` + one `work_item_delta` or `work_item_claim` (dynamic only) | Host injects work_items window (`wi_…`) |
| **B** | no `work_item_mode` | `status: in_progress` + one `validate_result_delta` (`#N label: outcome`); optional `progress` (`N/M`) | Full numbered list in **`plan`** |

- Pick **A or B** per milestone — do not mix on the same row. Mode A: omit `progress`, `validate_result_delta`, `validate_results`.
- Mark milestone **`done`** in a **later patch** after all items are complete (at most one `done` row per patch).
- Required: **`id`**, **`status`**. Omit empty optional fields. Host dedupes duplicate delta lines.

## Core rules

- If `[TASK_BOARD]` is empty and task is multi-step, call `init`.
- Patch every turn that completes one item on the active row (same turn as verify when applicable).
- Keep 3-12 milestones for most tasks.
- Cancel obsolete rows with **`task_board_prune`** instead of ignoring them.
- Finalize in the same turn as final user delivery.
- Loop safety is enforced by **tool rounds** per user message — not by a board step cap.

## Parent / child scope

- **Sub-agent (child):** **`[TASK_BOARD]`** = local `local_*` steps only.
  **`[TASK_BOARD_PARENT]`** is read-only (goal + findings + current milestone).
  Use **`task_board_sync_finding`** for breakthroughs; **do not** patch parent rows.
- **Lead (parent):** milestone scope only — no micromanaging child `local_*` rows.

## Milestone grouping

- Matrix/combinational goals: **3–8** grouped milestones (interaction form or meaningful dimension);
  do not expand to every atomic case.
- Short lists (≤5, independent acceptance): one item per row is OK.
- Long or repetitive lists (>5): batch by type/phase; keep **3–8** milestones.

## Final delivery (user summary)

When writing the **final summary** in assistant **`content`**:

- **Source of truth:** injected **`[TASK_BOARD]`** in this turn.
- Read outcome evidence on each **`done`** row under **All tasks** (full list per row).
- Build outcome tables / counts from board **`id`** and **`title`** — not from memory.
- If a **`done`** row has no outcome evidence in the inject → report as **unverified**; do not guess.
- Call **`finalize`** in the same turn as the final summary when every row is terminal.

## Profile guidance

Computer (with `action_verify`):

- Initialize when expected operation steps >3, or **>5** similar repetitive operations.
- Long lists (>5): **3–8** batched milestones; put the full target list in **`plan`** (Mode B) or **`work_items[]`** (Mode A) — not in **`title`**.
- **`title`** / **`validate_requirement`**: short batch scope with item numbers tied to **`plan`** (e.g. `#3–#7`).
- Cadence: `action_verify` → **`task_board_patch`** same turn when an item completes (see **Patch**).

Engineering profiles:

- Put command/test evidence in **`validate_result_delta`** while working.
- Put acceptance criteria in **`validate_requirement`**.

## Items input

Use **`items`** on **`task_board_init`** / **`task_board_replace`** / **`task_board_patch`**
(JSON array, or a JSON string encoding that array).

Injected **`[TASK_BOARD]`** snapshots may show a `board` array — host output only.

Each row must include non-empty **`id`**, **`title`**, and **`status`** on init/replace.
On **`patch`**, include **`title`** when adding a row or when updating status without an existing title.

Examples use **`function.name`** + **`function.arguments`** only (no `method`, no call `id` / `type`).

#### Example — `task_board_init`

```json
{
  "function": {
    "name": "task_board_init",
    "arguments": {
      "goal": "Ship feature X",
      "items": [
        {
          "id": "m1",
          "title": "Locate code",
          "status": "pending",
          "validate_requirement": "Tests pass after handler change"
        }
      ]
    }
  }
}
```

#### Example — `task_board_patch` (milestone done)

```json
{
  "function": {
    "name": "task_board_patch",
    "arguments": {
      "items": [
        {
          "id": "m1",
          "status": "done"
        }
      ]
    }
  }
}
```

#### Example — `task_board_patch` (extract delta only)

```json
{
  "function": {
    "name": "task_board_patch",
    "arguments": {
      "items": [
        {
          "id": "m2",
          "status": "in_progress",
          "extract_result_delta": "| id | name |\n| --- | --- |\n| 1 | foo |"
        }
      ]
    }
  }
}
```

#### Example — `task_board_patch` (progress only)

```json
{
  "function": {
    "name": "task_board_patch",
    "arguments": {
      "items": [{ "id": "m1", "status": "in_progress", "progress": "3/5" }]
    }
  }
}
```

#### Example — `task_board_patch` (Mode B — one item)

```json
{
  "function": {
    "name": "task_board_patch",
    "arguments": {
      "items": [
        {
          "id": "m1",
          "status": "in_progress",
          "progress": "2/5",
          "validate_result_delta": "#2 13823456789: User found (晓)"
        }
      ]
    }
  }
}
```

#### Example — `task_board_patch` (start row — status only)

```json
{
  "function": {
    "name": "task_board_patch",
    "arguments": {
      "items": [{ "id": "m1", "status": "in_progress" }]
    }
  }
}
```

## Computer + work_items (when enabled)

**Mode A** (see **Patch**). Init may inline **`work_items[]`** (≤200); inject shows a window + `N/M done`.

**Enumerated** — `work_item_delta` for one `wi_…` id:

```json
{
  "items": [{
    "id": "batch_apps",
    "status": "in_progress",
    "work_item_delta": {
      "id": "wi_conv_000003",
      "status": "done",
      "result_summary": "WeChat: main window visible"
    }
  }]
}
```

**Dynamic** (`dynamic_quota` required) — one `work_item_claim` or `work_item_delta` per patch:

```json
{
  "items": [{
    "id": "quota_greet",
    "status": "in_progress",
    "work_item_claim": {
      "target_key": "linkedin:ACoA…",
      "title": "张三 · 某公司",
      "status": "in_progress"
    }
  }]
}
```

**User file delivery** (xlsx/csv): final **`deliver_*` milestone** + host export; not per-item `MEDIA`.
