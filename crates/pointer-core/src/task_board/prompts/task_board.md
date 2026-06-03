### `task_board`

Session-scoped working memory for multi-step execution.

If work is multi-step, initialize early.
Single-step work may skip the board.

**Native flat tools** — call by tool name. **Do not** pass a `method` field in arguments; the host maps the tool name to the operation.

- **`task_board_init`**: set `goal`, optional `global_context`, and `items`.
- **`task_board_replace`**: replace full board.
- **`task_board_patch`**: merge one current row (**required** `status`; optional `progress`, deltas).
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

## `validate_*` vs `verify:report`

- **`verify:report`** (sidecar): validates a **single step** action (UI/click/type).
- **`validate_requirement` / `validate_result_delta`**: validates the **milestone outcome** while working.
- Injected **`[TASK_BOARD]`** shows recent **`validate_result_delta`** on the current row; completed milestones show full outcome evidence under **All tasks**.

Do not paste `verify:report` JSON into `validate_result_delta`.
Summarize observable outcome in one short markdown line.

## Patch cadence (delta fields)

On **`task_board_patch`**, each **`items`** row must include **`id`** and **`status`** (current milestone state: usually **`in_progress`** while working, **`done`** when complete).

**Optional** — **omit** when unchanged this call:

- **`progress`** — **replace** when present.
- **`validate_result_delta`** — append one new outcome line this turn.
- **`extract_result_delta`** — append one new extract line this turn.

Do **not** send empty strings or placeholders for unused optional fields.

When a step advances: **`status: in_progress`** + **`progress`** + **one** `validate_result_delta` in the same patch.

Host dedupes duplicate delta lines (warnings: `validate_results_duplicate_*`, …).

## Core rules

- If `[TASK_BOARD]` is empty and task is multi-step, call `init`.
- **One milestone, many patches:** a large or long-running milestone is updated with **many** `patch` calls over time — each call refreshes **`progress`** (replace) and appends **one** `validate_result_delta`. Do not wait until the end to patch once.
- **One row per call:** each `patch` has **one** object in **`items`**, and that row’s **`id`** must be the **current** task from **`[TASK_BOARD]`** (not other milestones).
- Each turn that completes a step on the active row: `patch` in the **same turn** with **one** new `validate_result_delta` line.
- Do **not** batch many rows to `done` in a single patch at the end.
- Mark `done` only after outcome evidence exists (deltas and/or action tools); **at most one** row `done` per patch.
- Keep 3-12 milestones for most tasks.
- Child agents must not patch parent rows directly.
- Finalize in the same turn as final user delivery.

## Final delivery (user summary)

When writing the **final summary** in assistant **`content`**:

- **Source of truth:** injected **`[TASK_BOARD]`** in this turn.
- Read outcome evidence on each **`done`** row under **All tasks** (full list per row).
- Build outcome tables / counts from board **`id`** and **`title`** — not from memory.
- If a **`done`** row has no outcome evidence in the inject → report as **unverified**; do not guess.
- Call **`finalize`** in the same turn as the final summary when every row is terminal.

## Profile guidance

Computer (with `verify_report`):

- Initialize when expected operation steps >3, or **>5** similar repetitive operations (enumerated targets or cycles).
- For large enumerations: **3–6 batched milestones** (not one row for the full set); full set in **`plan`** or **`extract_result_delta`** lines accumulated on one row.
- **Cadence:** `verify_report` (step) → `task_board_patch` (same turn): set **`progress=N/M`** and **one** `validate_result_delta` line for the step just verified.
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

Use **`items`** on **`task_board_init`** / **`task_board_replace`** / **`task_board_patch`**.

Each row must include non-empty **`id`** and **`title`** on init.

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
