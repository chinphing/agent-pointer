Report verify and repetition conclusion for host tier runtime.

This is a sidecar-only tool.
Do not use it as the root tool when another root tool is present.

Method:
- `report`

Required args:
- `action_result`: `pass` | `fail` | `n/a`
- `repetition_count`: non-negative integer
- `failure_cause`: required only when `action_result=fail`; one of `wrong_operation` | `precision_miss`

Rules:
- Mirror the same action_result and repetition_count used in `thoughts`.
- Set `failure_cause` only on fail; omit it on `pass` and `n/a`.
- Keep values concise and deterministic.
