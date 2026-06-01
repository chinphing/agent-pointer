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

## Row fields policy

Each row may include:

- `details`: execution plan + implementation details + key points.
- `progress`: partial progress for in-flight work.
- `validate`: final acceptance check only.
- `output`: concise result summary.

Do not overload `validate` with process details.
Put process details in `details`.

## Details format (recommended)

Use a short markdown table in `details`:

| step | action | key_points | risk | done_when |
| --- | --- | --- | --- | --- |
| 1 | ... | ... | ... | ... |

Keep lines short and actionable.
Avoid long prose.

## Progress format (recommended)

Keep `progress` concise and incremental.
Prefer checkpoint style, for example:

- `2/5 checkpoints done`
- `current: data migration`
- `next: run integration tests`

For matrix/combinational tasks,
record covered and remaining slices.

## Core rules

- If `[TASK_BOARD]` is empty and task is multi-step, call `init`.
- `patch` should update current task row first.
- Mark `done` only after observable evidence.
- Keep 3-12 milestones for most tasks.
- For repetitive/matrix work, group by meaningful slices.
- Child agents must not patch parent rows directly.
- Finalize in the same turn as final user delivery.
- If `retry_count >= 2`, do internal diagnosis before next patch.

## Profile guidance

Computer profile (with `verify_report`):

- Complexity gate: initialize when expected operation steps >3.
- First board-init round may skip `verify_report`.
- After init, run `verify_report` before `task_board_patch`.

Engineering profiles:

- Use test/command/file evidence.
- Update `validate` with final acceptance evidence.
- Coder complexity gate: initialize when expected scope is >=2 files or cross-module.

## Items input

Use **`items`** for milestone rows (`init` / `replace` / `patch`).
Do not use `rows` — host accepts it as an alias, but **`items`** is canonical.

Each row must include non-empty **`id`** and **`title`**.

`items` can be:

- a JSON array
- a JSON string that encodes that array

Single-row patch can use top-level fields:
`item_id` (or `id`) + row fields.

#### Example — initialize board

```json
{
  "function": {
    "name": "task_board_init",
    "arguments": {
      "goal": "Ship feature X",
      "items": [
        { "id": "m1", "title": "Locate code", "status": "pending" }
      ]
    }
  }
}
```

#### Example — patch with details/progress/validate

Example patch call:

```json
{
  "function": {
    "name": "task_board_patch",
    "arguments": {
      "items": [
        {
          "id": "m1",
          "status": "done",
          "details": "| step | action | key_points | risk | done_when |\n| --- | --- | --- | --- | --- |\n| 1 | update handler | keep API stable | medium | tests pass |",
          "progress": "all checkpoints complete",
          "validate": "Tests pass",
          "output": "Handler updated"
        }
      ]
    }
  }
}
```

#### Example — finalize when all rows are terminal

Call after the last row is **`done`** or **`cancelled`**:

```json
{
  "function": {
    "name": "task_board_finalize",
    "arguments": {}
  }
}
```
