### Scenario: generic

Fallback when no specific family matches. Compare before/after for any UI change implied by `goal` and `action`.

**Evidence:** **[Screen before action]** vs **[Screen after action]**.

**Order:** goal first → pointer **only on fail** (same as pointer click).

| Goal evidence | `action_result` | Pointer (fail only) | `failure_cause` |
| --- | --- | --- | --- |
| Supporting UI change | `pass` | skip | — |
| No clear or contradicting change | `fail` | center-hit | `wrong_operation` |
| No clear or contradicting change | `fail` | center-miss | `precision_miss` |

### Reasoning (hard rule)

Max **4 sentences**, plain prose — follow pointer click reasoning pattern.

**Pointer rule:** cursor on target alone ≠ pass; visible UI outcome required.

**Forbidden:** numbered CoT templates; JSON; repeating tool args.
