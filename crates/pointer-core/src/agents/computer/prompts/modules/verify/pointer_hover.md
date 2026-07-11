### Scenario: pointer hover

Judge whether hover revealed the expected UI (tooltip, popup preview, menu, highlight).

**Evidence:** **[Screen before action]** vs **[Screen after action]**.

**Order:** hover UI first → pointer **only when hover UI did not appear**.

| Goal evidence | `action_result` | Pointer (fail only) | `failure_cause` |
| --- | --- | --- | --- |
| Tooltip / popup / highlight visible | `pass` | skip | — |
| No hover UI change | `fail` | on target | `wrong_operation` |
| Wrong element reacted | `fail` | any | `wrong_operation` |

### Reasoning (hard rule)

Max **3 sentences**, plain prose — no numbered lists or section headers.

1. Before/after hover UI → pass if expected reveal; else fail.
2. On fail only: pointer on intended target or not → `failure_cause`.
3. Optional: loading if applicable.

**Forbidden:** numbered CoT templates; JSON; repeating tool args.
