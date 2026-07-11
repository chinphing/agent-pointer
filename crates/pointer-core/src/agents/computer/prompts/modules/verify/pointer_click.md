### Scenario: pointer click

Judge whether a click produced the UI change implied by `goal` and `action`.

**Evidence:** **[Screen before action]** vs **[Screen after action]**.

**Order:** goal first → pointer **only on fail**.

| Goal evidence | `action_result` | Pointer (fail only) | `failure_cause` |
| --- | --- | --- | --- |
| Supporting UI change | `pass` | skip | — |
| Contradicting change | `fail` | center-hit | `wrong_operation` |
| Contradicting change | `fail` | center-miss | `precision_miss` |
| No clear change | `fail` | center-hit | `wrong_operation` |
| No clear change | `fail` | center-miss | `precision_miss` |

**`loading_detected`:** spinner, progress bar, or partial load in after.

**`step_summary` (pass):** one sentence on the visible outcome.

### Reasoning (hard rule)

Max **4 sentences**, plain prose — no numbered lists or section headers.

1. Goal check on before/after → supporting / contradicting / no_clear_evidence.
2. Supporting → `pass`; stop — **do not** analyze pointer.
3. Fail → center-hit vs center-miss for `failure_cause`.
4. Optional: note loading if applicable.

**Forbidden:** "Analyze the Request"; JSON/`{`/`}`; repeating `submit_verify` args.

**Pointer rule:** cursor on target alone ≠ pass; visible UI outcome required. Exception: `mouse_move` — cursor at target = pass.
