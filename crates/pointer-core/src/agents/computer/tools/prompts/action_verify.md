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

Rules:
- Call only when the **newest** `[Recent desktop tool calls]` row shows **`verify: verifying`**.
- Do **not** call when that row shows **`verify: verified - *`** or **`verify: skipped`**.
- Host closes the **newest open** history row on pass/fail/n/a; **`pending`** keeps the row open.
- Mirror the same action_result and repetition_count from your internal Verify / Repetition conclusion.
- Set `failure_cause` only on fail; omit it on `pass`, `pending`, and `n/a`.
- Keep values concise and deterministic.
