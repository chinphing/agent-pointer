### Scenario: modified click

Judge whether Ctrl/Cmd/Shift+click produced expected multi-selection or special action.

**Evidence:** **[Screen before action]** vs **[Screen after action]**.

**Order:** same as pointer click — goal first → pointer **only on fail**.

| Outcome | `action_result` | Pointer (fail only) | `failure_cause` |
| --- | --- | --- | --- |
| Multiple items selected / modified-click effect | `pass` | skip | — |
| Single or unchanged selection | `fail` | center-hit | `wrong_operation` |
| Single or unchanged selection | `fail` | center-miss | `precision_miss` |

**`loading_detected`:** UI still settling.

### Reasoning (hard rule)

Max **4 sentences**, plain prose — same structure as pointer click.

1. Before/after selection or modified-click effect vs `goal`.
2. Pass → stop; fail → center-hit vs center-miss.
3. Optional: loading.

**Forbidden:** numbered CoT templates; JSON; repeating tool args.
