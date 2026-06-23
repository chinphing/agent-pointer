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
- **`task_board_patch`**: merge one current row — **one atomic work item** per call (see below).
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

## Patch granularity (one atomic work item)

Every **`task_board_patch`** records progress for **one atomic work item** inside the **current milestone** — not the whole board, not multiple milestones, not many items in one call.

**Always:**

- **One object** in **`items`** — the **current** milestone row from **`[TASK_BOARD]`**.
- **One item outcome** per patch (see mode below).
- Mark the milestone **`done`** in a **separate** patch after **all** items in that milestone are complete.

### Mode A — host work_items injection

Use when the current row has **`work_item_mode`** (`enumerated` or `dynamic`).

- Host stores atomic rows in **`work_items.db`**; inject shows a **window** of ids + titles + status.
- Each patch: **`status: in_progress`** (while working) + **one** of:
  - **`work_item_delta`** — update **one** host id from inject (`wi_…`).
  - **`work_item_claim`** — dynamic mode only; then a later patch uses **`work_item_delta`**.
- **Do not** use **`progress`**, **`validate_result_delta`**, or **`validate_results`** on these rows.

### Mode B — no work_items injection (milestone carries the item list)

Use when the row has **no** **`work_item_mode`**, but the milestone clearly enumerates items (in **`plan`**, **`validate_requirement`**, or numbered scope in **`title`**).

- The **full numbered list** lives in **`plan`** (or one-time **`extract_result_delta`**).
- Each patch: **`status: in_progress`** + **one** **`validate_result_delta`** line for **one** numbered item (e.g. `#3 微信: opened`).
- Optional **`progress`** = `N/M` for items completed in that milestone.
- Mark milestone **`done`** only when **every** numbered item in **`plan`** / requirement has a matching delta line.

Choose **Mode A or B** for a milestone — do not mix delta styles on the same row.

## Patch cadence (Mode B delta fields)

On **`task_board_patch`**, each **`items`** row must include **`id`** and **`status`** (current milestone state: usually **`in_progress`** while working, **`done`** when complete).

**Optional** — **omit** when unchanged this call:

- **`progress`** — **replace** when present.
- **`validate_result_delta`** — append **one** new outcome line for **one** atomic item this turn.
- **`extract_result_delta`** — append one new extract line this turn.

Do **not** send empty strings or placeholders for unused optional fields.

When one atomic item advances: **`status: in_progress`** + **one** `validate_result_delta` in the same patch (and **`progress`** when useful).

Host dedupes duplicate delta lines (warnings: `validate_results_duplicate_*`, …).

## Core rules

- If `[TASK_BOARD]` is empty and task is multi-step, call `init`.
- **Patch = one atomic item:** never batch multiple item outcomes or multiple milestones in one `patch`.
- **One milestone, many patches:** a large milestone is updated with **many** `patch` calls — each records **one** item (Mode A or B above).
- **One row per call:** each `patch` has **one** object in **`items`**, and that row’s **`id`** must be the **current** task from **`[TASK_BOARD]`**.
- Each turn that completes **one** item on the active row: `patch` in the **same turn** with that item’s delta only.
- Do **not** batch many items or rows to `done` in a single patch at the end.
- Mark `done` only after every item in that milestone is recorded (Mode A: all work_items terminal; Mode B: all numbered deltas present).
- **At most one** milestone `done` per patch.
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

- Initialize when expected operation steps >3, or **>5** similar repetitive operations (enumerated targets or cycles).
- For large enumerations: **3–6 batched milestones** (not one row for the full set); full set in **`plan`** or **`extract_result_delta`** lines accumulated on one row.
- **Cadence:** `action_verify` (step) → `task_board_patch` (same turn): set **`progress=N/M`** and **one** `validate_result_delta` line for the step just verified.
- While a batch row is `in_progress`: patch **every turn** that advances progress (do not wait until the batch ends).
- **List / enumeration rows:** the **full numbered target list** (all names/URLs/labels) lives in **`plan`** or in **`extract_result_delta`** once — not in **`title`** (too short for large sets).
  **`title`** / **`validate_requirement`**: short batch scope with **the same item numbers** as that list (e.g. `#3–#7` or `#3 微信, #4 钉钉`); do not use ordinals alone ("items 1–5") without numbers tied to **`plan`**.
  Each **`validate_result_delta`** line: **number + label** matching verify (e.g. `#3 微信: opened`), not only `3/10: ok`.
  Mark **`done`** only when every numbered target in that row's **`plan`** / requirement is covered by deltas.
- When a batch row is complete: **one** patch with `status: done` for **that row only**.
- **Never** defer all batch `done` updates to one patch after the full enumeration is finished.

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

#### Example — `task_board_patch` (delta while working)

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

#### Example — `task_board_patch` (one delta only)

```json
{
  "function": {
    "name": "task_board_patch",
    "arguments": {
      "items": [
        {
          "id": "m1",
          "status": "in_progress",
          "validate_result_delta": "#3 13734567890: User found (夢回 80)"
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

## Computer + work_items (Mode A, when enabled)

When a milestone has **`work_item_mode`**, follow **Mode A** above — **one `work_item_delta` or `work_item_claim` per patch**.

**Enumerated** (`work_item_mode: enumerated`):

- Init may inline **`work_items[]`** (≤200) on the milestone; host stores rows in **`work_items.db`**.
- Each patch: **`status: in_progress`** + **`work_item_delta`** for **one** atomic row.

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

**Dynamic** (`work_item_mode: dynamic`, **`dynamic_quota`** required):

- **One claim OR one delta per patch** — claim creates a row; a later patch closes it with delta.

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

**Milestones without `work_item_mode`:** use **Mode B** — numbered items in **`plan`** + **`validate_result_delta`**, one item per patch.

**Inject**: `[TASK_BOARD]` shows a **window** of recent work_items + `N/M done`; full list is not in prompt.

**Do not** send `progress`, `validate_result_delta`, or `validate_results` on **Mode A** rows.

**User file delivery** (xlsx/csv): final **`deliver_*` milestone** + host export (P3); not per-item `MEDIA`.
