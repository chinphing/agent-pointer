---
schema:
  type: object
  properties:
    action_result:
      type: string
      enum:
        - pass
        - fail
        - pending
        - n/a
    repetition_count:
      type: integer
      minimum: 0
    failure_cause:
      type: string
    step_summary:
      type: string
  required:
    - action_result
    - repetition_count
  additionalProperties: true
---

Report verify and repetition conclusion for host tier runtime.

Sidecar-only flat tool: call **`action_verify`** by name (no `method` field).

Do not use it as the root tool when another root tool is present.

Required args:
- `action_result`: `pass` | `fail` | `pending` (`n/a` kept for backward compatibility)
- `repetition_count`: non-negative integer
- `failure_cause`: required only when `action_result=fail`; one of `wrong_operation` | `precision_miss`

Optional args:
- `step_summary`: **required only when `action_result=pass`**; omit on `fail`, `pending`, and `n/a`

## `step_summary` (pass only)

One short line (≤400 chars) for chat history and final rollup when no task board.

Anchor on the **user task** (latest real user request; if `[TASK_BOARD]` exists, its **goal** and **current milestone**).

Format (example):

`Toward <user task>: <this step> → <observable UI outcome>`

Rules:
- State how this **pass** advances the **user task**, not only the tool `goal` from the last desktop row.
- Do not paste internal Verify templates or overlay index lists.
- On `fail` / `pending` / `n/a`: **omit** `step_summary` (host ignores it if sent).

## Verify reporting

Rules:
- Call only when the **newest** `[Recent desktop tool calls]` row shows **`verify: verifying`**.
- Do **not** call when that row shows **`verify: verified - *`** or **`verify: skipped`**.
- Host closes the **newest open** history row on pass/fail/n/a; **`pending`** keeps the row open.
- Mirror the same `action_result` and `repetition_count` from your internal Verify / Repetition conclusion.
- Set `failure_cause` only on fail; omit it on `pass`, `pending`, and `n/a`.
- Keep values concise and deterministic.
