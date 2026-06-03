---
schema:
  type: object
  properties:
    method:
      type: string
      enum:
        - init
        - replace
        - patch
        - prune
        - finalize
        - sync_finding
        - check_deps
    goal:
      type: string
    items: {}
    global_context: {}
    ids:
      type: array
      items:
        type: string
    item_id:
      type: string
    finding:
      type: string
    expected_total:
      type: integer
      minimum: 1
    _conversation_id:
      type: string
  required:
    - method
  additionalProperties: true
---

### `task_board`

Session-scoped working memory for multi-step execution.

If work is multi-step, initialize early.
Single-step work may skip the board.

Methods:

- `init`: set `goal`, optional `global_context`, and `items`.
- `replace`: replace full board.
- `patch`: merge rows by `id`.
- `prune`: cancel pending rows.
- `finalize`: set board complete after all rows are terminal.
- `sync_finding`: child board sync to parent findings.
- `check_deps`: inspect dependency readiness for one row.

Tool result is compact.
Treat injected `[TASK_BOARD]` as source of truth.

Row status:
`pending`, `ready`, `in_progress`, `done`, `cancelled`, `failed`.

## Row fields (v3)

Each row may include:

| Field | Patch | Content |
| --- | --- | --- |
| `plan` | replace | How to execute (markdown). |
| `checkpoint` | replace | Coarse position only (`cycle=3/10 \| phase=… \| next=…`). Update rarely. |
| `validate_requirement` | replace | Milestone **outcome** acceptance criteria (markdown). |
| `validate_results` | **append only** | Outcome evidence snippets (markdown lines). |
| `extract_requirement` | replace | What to extract (fields/scope, markdown). Optional. |
| `extract_results` | **append only** | Extracted facts (markdown table/list). Optional. |

User-facing delivery belongs in **assistant `content`**, not board row fields.

## `validate_*` vs `verify:report`

- **`verify:report`** (sidecar): validates a **single step** action (UI/click/type).
- **`validate_requirement` / `validate_results`**: validates the **milestone outcome**.

Do not paste `verify:report` JSON into `validate_results`.
Summarize observable outcome in one short markdown line.

## Append rules

- `validate_results`: pass a **string** (one snippet) or **string array** (several snippets). Host appends; never shortens history on patch.
- **One line per step** — e.g. `3/10: <target> - <outcome>`. Do **not** re-paste earlier lines or full cumulative tables.
- Host dedupes duplicate lines and drops redundant blocks (warnings: `validate_results_duplicate_*`, `validate_results_partial_dedup`).
- `extract_results`: same append semantics (host dedupes lines the same way).
- Fine-grained progress: append `validate_results`; change `checkpoint` only when cycle/phase/next shifts (`progress=N/M`).

## Core rules

- If `[TASK_BOARD]` is empty and task is multi-step, call `init`.
- `patch` should update the **current** task row only (one row per call).
- **Report progress during execution** — not only when the whole job is finished.
- Each turn that completes a step on the active row: `patch` in the **same turn** with **one** new `validate_results` line.
- Do **not** batch many rows to `done` in a single patch at the end.
- Mark `done` only after `validate_results` has evidence (or action tools ran); **at most one** row `done` per patch.
- Keep 3-12 milestones for most tasks.
- Child agents must not patch parent rows directly.
- Finalize in the same turn as final user delivery.

## Final delivery (user summary)

When writing the **final summary** in assistant **`content`**:

- **Source of truth:** injected **`[TASK_BOARD]`** in this turn.
- Read **`validate_results`** on each **`done`** row under **All tasks** (full list per row).
- Build outcome tables / counts from board **`id`** and **`title`** — not from memory.
- Missing **`validate_results`** on a **`done`** row → report as **unverified**; do not guess.
- Call **`finalize`** in the same turn as the final summary when every row is terminal.

## Profile guidance

Computer (with `verify_report`):

- Initialize when expected operation steps >3, or **>5** similar repetitive operations (enumerated targets or cycles).
- For large enumerations: **3–6 batched milestones** (not one row for the full set); full set in `plan` or `extract_results`.
- **Cadence:** `verify_report` (step) → `task_board_patch` (same turn): append **one** `validate_results` line for the step just verified; update `checkpoint=progress=N/M`.
- While a batch row is `in_progress`: patch **every turn** that advances progress (do not wait until the batch ends).
- When a batch row is complete: **one** patch with `status: done` for **that row only** (may include a short final line in `validate_results`).
- **Never** defer all batch `done` updates to one patch after the full enumeration is finished.

Engineering profiles:

- Put command/test evidence in `validate_results` (append).
- Put acceptance criteria in `validate_requirement`.

## Items input

Use **`items`** for milestone rows (`init` / `replace` / `patch`).

Each row must include non-empty **`id`** and **`title`**.

#### Example — initialize

```json
{
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
```

#### Example — patch (append evidence + done)

```json
{
  "items": [
    {
      "id": "m1",
      "status": "done",
      "validate_results": "- cargo test -p foo: 12 passed"
    }
  ]
}
```

#### Example — extract batch

```json
{
  "items": [
    {
      "id": "m2",
      "extract_results": "| id | name |\n| --- | --- |\n| 1 | foo |"
    }
  ]
}
```
