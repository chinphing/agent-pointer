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
- `extract_results`: same append semantics.
- Fine-grained progress: append `validate_results`; change `checkpoint` only when cycle/phase/next shifts.

## Core rules

- If `[TASK_BOARD]` is empty and task is multi-step, call `init`.
- `patch` should update the current task row first.
- Mark `done` only after `validate_results` has evidence (or action tools ran).
- Keep 3-12 milestones for most tasks.
- Child agents must not patch parent rows directly.
- Finalize in the same turn as final user delivery.

## Profile guidance

Computer (with `verify_report`):

- Initialize when expected operation steps >3.
- After init: `verify_report` (step) then `task_board_patch` (milestone).
- On milestone done: append `validate_results`, then `status: done`.

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
